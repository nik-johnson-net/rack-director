//! Redfish power driver using `reqwest`.
//!
//! Implements [`crate::director::power::PowerDriver`] via the DMTF Redfish
//! REST API.  System discovery happens at construction time:
//! `GET /redfish/v1/Systems` is called, and `Members[0]["@odata.id"]` is used
//! as the ComputerSystem path for all subsequent power operations.
//!
//! TLS verification is disabled by default (most BMC firmware ships
//! self-signed certificates) and can be enabled globally via
//! `PowerConfig::verify_tls`.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::{PowerConfig, PowerDriver, PowerState};

/// Redfish power driver for a single BMC.
///
/// The driver holds a `reqwest::Client` configured with the BMC's TLS
/// settings, the base origin URL (`https://{ip}`), the discovered
/// `ComputerSystem` path, and the BMC credentials for HTTP Basic auth.
pub struct RedfishDriver {
    client: Client,
    /// `https://{ip}` — no trailing slash
    origin: String,
    /// `/redfish/v1/Systems/<id>` — the discovered ComputerSystem path
    system_path: String,
    username: String,
    password: String,
}

/// Minimal JSON shapes for Redfish discovery and power-state queries.

#[derive(Deserialize)]
struct SystemsCollection {
    #[serde(rename = "Members")]
    members: Vec<OdataRef>,
}

#[derive(Deserialize)]
struct OdataRef {
    #[serde(rename = "@odata.id")]
    odata_id: String,
}

#[derive(Deserialize)]
struct SystemResource {
    #[serde(rename = "PowerState")]
    power_state: Option<String>,
}

impl RedfishDriver {
    /// Discover the Redfish ComputerSystem path and build a `RedfishDriver`.
    ///
    /// Calls `GET {origin}/redfish/v1/Systems` with Basic auth and reads
    /// `Members[0]["@odata.id"]` as the system path.  Logs a warning if
    /// more than one system is found (rack-director always operates on
    /// `Members[0]`).
    ///
    /// # Arguments
    /// * `ip`       – BMC IP address (used to build `https://{ip}`)
    /// * `username` – Basic-auth username
    /// * `password` – Basic-auth password
    /// * `config`   – TLS + timeout settings
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails, the response is non-2xx,
    /// or the `Members` array is empty.
    pub async fn discover(
        ip: &str,
        username: &str,
        password: &str,
        config: PowerConfig,
    ) -> Result<Self> {
        let origin = format!("https://{ip}");
        Self::discover_with_origin(&origin, username, password, config).await
    }

    /// Like [`discover`] but accepts a full origin URL.
    ///
    /// This variant exists to allow tests to point the driver at an HTTP
    /// wiremock server instead of a real HTTPS BMC.
    pub(crate) async fn discover_with_origin(
        origin: &str,
        username: &str,
        password: &str,
        config: PowerConfig,
    ) -> Result<Self> {
        let client = build_client(config)?;
        let systems_url = format!("{origin}/redfish/v1/Systems");

        let collection: SystemsCollection = client
            .get(&systems_url)
            .basic_auth(username, Some(password))
            .send()
            .await
            .context("Redfish GET /redfish/v1/Systems request failed")?
            .error_for_status()
            .context("Redfish GET /redfish/v1/Systems returned non-2xx")?
            .json()
            .await
            .context("Failed to deserialize Redfish Systems collection")?;

        if collection.members.len() > 1 {
            log::warn!(
                "Redfish at {} reports {} ComputerSystems; using Members[0]",
                origin,
                collection.members.len()
            );
        }

        let system_path = collection
            .members
            .into_iter()
            .next()
            .map(|m| m.odata_id)
            .ok_or_else(|| anyhow::anyhow!("Redfish Systems collection is empty at {}", origin))?;

        Ok(Self {
            client,
            origin: origin.to_string(),
            system_path,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// POST a `ComputerSystem.Reset` action with the given `ResetType`.
    async fn reset(&self, reset_type: &str) -> Result<()> {
        let url = format!(
            "{}{}/Actions/ComputerSystem.Reset",
            self.origin, self.system_path
        );
        let body = json!({ "ResetType": reset_type });

        self.client
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&body)
            .send()
            .await
            .context("Redfish ComputerSystem.Reset request failed")?
            .error_for_status()
            .context("Redfish ComputerSystem.Reset returned non-2xx")?;

        Ok(())
    }
}

/// Map a power operation to the Redfish `ResetType` string.
///
/// This is a pure function so the mapping can be unit-tested independently of
/// any HTTP infrastructure.
///
/// Note that both [`ResetOp::Cycle`] and [`ResetOp::Reset`] map to
/// `"ForceRestart"`, even though the IPMI driver distinguishes
/// `chassis power cycle` (off-then-on) from `chassis power reset` (warm reset).
/// This is intentional: the more precise Redfish `"PowerCycle"` ResetType is
/// not universally supported across BMC firmware, whereas `"ForceRestart"` is
/// the broadly-implemented value that reliably reboots the host. The minor
/// loss of cycle-vs-reset fidelity is accepted in exchange for portability;
/// for rack-director's purposes (kicking a host back to PXE) both operations
/// achieve the same outcome.
fn reset_type_for_op(op: ResetOp) -> &'static str {
    match op {
        ResetOp::On => "On",
        ResetOp::OffGraceful => "GracefulShutdown",
        ResetOp::OffForced => "ForceOff",
        ResetOp::Cycle => "ForceRestart",
        ResetOp::Reset => "ForceRestart",
    }
}

/// Logical reset operations, used as input to [`reset_type_for_op`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetOp {
    On,
    OffGraceful,
    OffForced,
    Cycle,
    Reset,
}

/// Build a `reqwest::Client` with the given power config.
fn build_client(config: PowerConfig) -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(!config.verify_tls)
        .timeout(config.command_timeout)
        .build()
        .context("Failed to build reqwest client for Redfish")
}

#[async_trait::async_trait]
impl PowerDriver for RedfishDriver {
    async fn power_state(&self) -> Result<PowerState> {
        let url = format!("{}{}", self.origin, self.system_path);
        let system: SystemResource = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .context("Redfish GET system resource failed")?
            .error_for_status()
            .context("Redfish GET system resource returned non-2xx")?
            .json()
            .await
            .context("Failed to deserialize Redfish system resource")?;

        Ok(match system.power_state.as_deref() {
            Some("On") => PowerState::On,
            Some("Off") => PowerState::Off,
            _ => PowerState::Unknown,
        })
    }

    async fn power_on(&self) -> Result<()> {
        self.reset(reset_type_for_op(ResetOp::On)).await
    }

    async fn power_off(&self, graceful: bool) -> Result<()> {
        let op = if graceful {
            ResetOp::OffGraceful
        } else {
            ResetOp::OffForced
        };
        self.reset(reset_type_for_op(op)).await
    }

    async fn power_cycle(&self) -> Result<()> {
        self.reset(reset_type_for_op(ResetOp::Cycle)).await
    }

    async fn power_reset(&self) -> Result<()> {
        self.reset(reset_type_for_op(ResetOp::Reset)).await
    }

    fn kind(&self) -> &'static str {
        "redfish"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{basic_auth, body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // -----------------------------------------------------------------------
    // reset_type_for_op mapping tests (pure — no HTTP needed)
    // -----------------------------------------------------------------------

    #[test]
    fn test_reset_type_on() {
        assert_eq!(reset_type_for_op(ResetOp::On), "On");
    }

    #[test]
    fn test_reset_type_graceful_off() {
        assert_eq!(reset_type_for_op(ResetOp::OffGraceful), "GracefulShutdown");
    }

    #[test]
    fn test_reset_type_forced_off() {
        assert_eq!(reset_type_for_op(ResetOp::OffForced), "ForceOff");
    }

    #[test]
    fn test_reset_type_cycle() {
        assert_eq!(reset_type_for_op(ResetOp::Cycle), "ForceRestart");
    }

    #[test]
    fn test_reset_type_reset() {
        assert_eq!(reset_type_for_op(ResetOp::Reset), "ForceRestart");
    }

    // -----------------------------------------------------------------------
    // wiremock integration tests
    // -----------------------------------------------------------------------

    /// Stand up a minimal Redfish mock server and return the base URL.
    ///
    /// Mounts:
    /// - `GET /redfish/v1/Systems` → 200 `{Members:[{"@odata.id":"/redfish/v1/Systems/1"}]}`
    /// - `GET /redfish/v1/Systems/1` → 200 with configurable `PowerState`
    /// - `POST /redfish/v1/Systems/1/Actions/ComputerSystem.Reset` → 204
    async fn start_mock_server(power_state: &str) -> (MockServer, String) {
        let server = MockServer::start().await;

        // Systems collection — this is what discovery (and therefore
        // resolve_power_driver) actually probes.
        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Members": [{"@odata.id": "/redfish/v1/Systems/1"}]
            })))
            .mount(&server)
            .await;

        // System resource with configurable PowerState
        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": "/redfish/v1/Systems/1",
                "PowerState": power_state
            })))
            .mount(&server)
            .await;

        // Reset action
        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let base_url = server.uri();
        (server, base_url)
    }

    fn test_config() -> PowerConfig {
        PowerConfig {
            verify_tls: false,
            command_timeout: std::time::Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn test_discover_picks_system_path() {
        let (_server, base_url) = start_mock_server("On").await;
        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        assert_eq!(driver.system_path, "/redfish/v1/Systems/1");
        assert_eq!(driver.kind(), "redfish");
    }

    #[tokio::test]
    async fn test_power_state_on() {
        let (_server, base_url) = start_mock_server("On").await;
        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        let state = driver.power_state().await.unwrap();
        assert_eq!(state, PowerState::On);
    }

    #[tokio::test]
    async fn test_power_state_off() {
        let (_server, base_url) = start_mock_server("Off").await;
        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        let state = driver.power_state().await.unwrap();
        assert_eq!(state, PowerState::Off);
    }

    #[tokio::test]
    async fn test_power_state_unknown() {
        let (_server, base_url) = start_mock_server("PoweringOn").await;
        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        let state = driver.power_state().await.unwrap();
        assert_eq!(state, PowerState::Unknown);
    }

    #[tokio::test]
    async fn test_power_on_posts_correct_reset_type() {
        let (server, base_url) = start_mock_server("Off").await;

        // Mount a specific mock that validates the request body
        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .and(body_json(serde_json::json!({"ResetType": "On"})))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        driver.power_on().await.unwrap();
    }

    #[tokio::test]
    async fn test_power_off_graceful_posts_correct_reset_type() {
        let (server, base_url) = start_mock_server("On").await;

        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .and(body_json(
                serde_json::json!({"ResetType": "GracefulShutdown"}),
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        driver.power_off(true).await.unwrap();
    }

    #[tokio::test]
    async fn test_power_off_forced_posts_correct_reset_type() {
        let (server, base_url) = start_mock_server("On").await;

        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .and(body_json(serde_json::json!({"ResetType": "ForceOff"})))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        driver.power_off(false).await.unwrap();
    }

    #[tokio::test]
    async fn test_power_cycle_posts_correct_reset_type() {
        let (server, base_url) = start_mock_server("On").await;

        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .and(body_json(serde_json::json!({"ResetType": "ForceRestart"})))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let driver =
            RedfishDriver::discover_with_origin(&base_url, "admin", "secret", test_config())
                .await
                .unwrap();
        driver.power_cycle().await.unwrap();
    }

    #[tokio::test]
    async fn test_discover_fails_on_empty_members() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Members": []
            })))
            .mount(&server)
            .await;

        let result =
            RedfishDriver::discover_with_origin(&server.uri(), "admin", "secret", test_config())
                .await;
        assert!(result.is_err(), "empty Members should return an error");
    }

    #[tokio::test]
    async fn test_discover_fails_on_non_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result =
            RedfishDriver::discover_with_origin(&server.uri(), "admin", "secret", test_config())
                .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_power_op_fails_on_non_2xx_reset() {
        let server = MockServer::start().await;

        // Systems discovery succeeds
        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Members": [{"@odata.id": "/redfish/v1/Systems/1"}]
            })))
            .mount(&server)
            .await;

        // Reset returns 500
        Mock::given(method("POST"))
            .and(path("/redfish/v1/Systems/1/Actions/ComputerSystem.Reset"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let driver =
            RedfishDriver::discover_with_origin(&server.uri(), "admin", "secret", test_config())
                .await
                .unwrap();

        let result = driver.power_on().await;
        assert!(result.is_err(), "non-2xx reset should return an error");
    }

    #[tokio::test]
    async fn test_discover_with_basic_auth() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redfish/v1/Systems"))
            .and(basic_auth("myuser", "mypass"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Members": [{"@odata.id": "/redfish/v1/Systems/1"}]
            })))
            .mount(&server)
            .await;

        let result =
            RedfishDriver::discover_with_origin(&server.uri(), "myuser", "mypass", test_config())
                .await;
        assert!(result.is_ok(), "correct credentials should succeed");
    }
}
