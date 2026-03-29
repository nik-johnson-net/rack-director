use anyhow::Result;
use serde::Serialize;

use crate::device_attributes::DeviceAttributes;
use crate::disk_layout::DiskLayout;

pub use crate::device_attributes::BmcConfig;
pub use crate::poll_action::{PollAction, PollResponse};

#[derive(Serialize)]
struct UpdateAttributesPayload<'a> {
    uuid: &'a str,
    attributes: &'a DeviceAttributes,
}

#[derive(Serialize)]
struct ActionStatusPayload<'a> {
    uuid: &'a str,
}

#[derive(Serialize)]
struct ActionFailedPayload<'a> {
    uuid: &'a str,
    error_message: &'a str,
}

/// HTTP client for rack-director's CNC (command-and-control) endpoints.
///
/// Shared between rack-agent and rack-simulator to ensure wire protocol parity.
/// Construct with [`CncClient::new`] and use the async methods to communicate
/// with a running rack-director instance. All methods return `anyhow::Result<T>`.
pub struct CncClient {
    client: reqwest::Client,
    url: String,
}

impl CncClient {
    /// Create a new client.
    ///
    /// `url` is the base URL of the rack-director instance, e.g.
    /// `http://rack-director:3000`. The `/cnc/` path prefix is appended
    /// automatically by each method.
    pub fn new(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: url.to_string(),
        }
    }

    /// Upload device hardware attributes to rack-director.
    ///
    /// Sends a `POST /cnc/update_attributes` with the device UUID and its
    /// current `DeviceAttributes`. Returns an error if the server responds
    /// with a non-2xx status code.
    pub async fn update_attributes(&self, uuid: &str, attributes: &DeviceAttributes) -> Result<()> {
        let payload = UpdateAttributesPayload { uuid, attributes };

        let response = self
            .client
            .post(format!("{}/cnc/update_attributes", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to update attributes: {}", response.status());
        }

        Ok(())
    }

    /// Report that the current action completed successfully.
    ///
    /// Sends a `POST /cnc/action_success` with the device UUID. rack-director
    /// will advance the device's provisioning plan to the next action.
    pub async fn action_success(&self, uuid: &str) -> Result<()> {
        let payload = ActionStatusPayload { uuid };

        let response = self
            .client
            .post(format!("{}/cnc/action_success", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to report action success: {}", response.status());
        }

        Ok(())
    }

    /// Report that the current action failed.
    ///
    /// Sends a `POST /cnc/action_failed` with the device UUID and a
    /// human-readable error message. rack-director will mark the plan as failed.
    pub async fn action_failed(&self, uuid: &str, error_message: &str) -> Result<()> {
        let payload = ActionFailedPayload {
            uuid,
            error_message,
        };

        let response = self
            .client
            .post(format!("{}/cnc/action_failed", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to report action failure: {}", response.status());
        }

        Ok(())
    }

    /// Fetch BMC configuration for the given device.
    ///
    /// Returns:
    /// - `Ok(Some(config))` on HTTP 200 — configuration exists.
    /// - `Ok(None)` on HTTP 404 — no BMC configuration is set for this device.
    /// - `Err(...)` for network errors, server errors, or unexpected status codes.
    pub async fn get_bmc_config(&self, uuid: &str) -> Result<Option<BmcConfig>> {
        let response = self
            .client
            .get(format!("{}/cnc/devices/{}/bmc_config", self.url, uuid))
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let config = response.json::<BmcConfig>().await?;
                Ok(Some(config))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => {
                anyhow::bail!("Failed to fetch BMC config: {}", status)
            }
        }
    }

    /// Fetch the resolved disk layout for the given device.
    ///
    /// rack-director resolves platform labels to concrete device paths before
    /// returning this response, so the agent receives a fully-resolved layout
    /// with no label references.
    ///
    /// Returns:
    /// - `Ok(layout)` on HTTP 200.
    /// - `Err(...)` on any other status, with the response body included in the
    ///   error message for easier debugging.
    pub async fn get_disk_layout(&self, uuid: &str) -> Result<DiskLayout> {
        let response = self
            .client
            .get(format!("{}/cnc/devices/{}/disk_layout", self.url, uuid))
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let layout = response.json::<DiskLayout>().await?;
                Ok(layout)
            }
            status => {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("Failed to fetch disk layout: {} - {}", status, body)
            }
        }
    }

    /// Poll rack-director for a pending action for the given device UUID.
    ///
    /// Returns:
    /// - `Ok(Some(response))` on HTTP 200 — an action is waiting.
    /// - `Ok(None)` on HTTP 204 — no active plan or no remaining actions.
    /// - `Err(...)` for network errors or unexpected status codes.
    pub async fn poll(&self, uuid: &str) -> Result<Option<PollResponse>> {
        let response = self
            .client
            .get(format!("{}/cnc/poll", self.url))
            .query(&[("uuid", uuid)])
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let msg = response.json::<PollResponse>().await?;
                Ok(Some(msg))
            }
            reqwest::StatusCode::NO_CONTENT => Ok(None),
            status => {
                anyhow::bail!("Unexpected status from poll endpoint: {}", status)
            }
        }
    }

    /// Fetch an iPXE boot script from rack-director.
    ///
    /// Used by rack-simulator to simulate PXE firmware behaviour. Optional
    /// `uuid` and `mac` query parameters are appended when provided.
    ///
    /// Returns:
    /// - `Ok(script)` on HTTP 2xx — the iPXE script body as a `String`.
    /// - `Err(...)` on any non-2xx response.
    pub async fn get_ipxe_script(&self, uuid: Option<&str>, mac: Option<&str>) -> Result<String> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(u) = uuid {
            params.push(("uuid", u));
        }
        if let Some(m) = mac {
            params.push(("mac", m));
        }

        let response = self
            .client
            .get(format!("{}/cnc/ipxe", self.url))
            .query(&params)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} from /cnc/ipxe: {}", status, body);
        }

        let body = response.text().await?;
        Ok(body)
    }

    /// Fetch an agent image file (kernel or initramfs) from rack-director.
    ///
    /// Used by rack-simulator to verify that agent images are accessible.
    /// `filename` is the bare filename, e.g. `"vmlinuz"` or `"initramfs.img"`.
    ///
    /// Returns:
    /// - `Ok(bytes)` on HTTP 2xx.
    /// - `Err(...)` on any non-2xx response.
    pub async fn get_agent_image(&self, filename: &str) -> Result<Vec<u8>> {
        let url = format!("{}/cnc/agent-images/{}", self.url, filename);

        let response = self.client.get(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} from {}: {}", status, url, body);
        }

        let bytes = response.bytes().await?.to_vec();
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── update_attributes ────────────────────────────────────────────────────

    /// Posting attributes with a 200 OK response succeeds.
    #[tokio::test]
    async fn test_update_attributes_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/update_attributes")
            .with_status(200)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let attrs = DeviceAttributes::default();
        let result = client.update_attributes("test-uuid", &attrs).await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    /// A 500 response from update_attributes returns an error.
    #[tokio::test]
    async fn test_update_attributes_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/update_attributes")
            .with_status(500)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let attrs = DeviceAttributes::default();
        let result = client.update_attributes("test-uuid", &attrs).await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    // ── action_success ───────────────────────────────────────────────────────

    /// Posting action_success with a 200 OK response succeeds.
    #[tokio::test]
    async fn test_action_success_ok() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/action_success")
            .with_status(200)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.action_success("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    /// A 422 response from action_success returns an error.
    #[tokio::test]
    async fn test_action_success_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/action_success")
            .with_status(422)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.action_success("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("422"));
    }

    // ── action_failed ────────────────────────────────────────────────────────

    /// Posting action_failed with a 200 OK response succeeds.
    #[tokio::test]
    async fn test_action_failed_ok() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/action_failed")
            .with_status(200)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client
            .action_failed("test-uuid", "something went wrong")
            .await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    /// A 500 response from action_failed returns an error.
    #[tokio::test]
    async fn test_action_failed_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/cnc/action_failed")
            .with_status(500)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.action_failed("test-uuid", "oops").await;

        mock.assert_async().await;
        assert!(result.is_err());
    }

    // ── get_bmc_config ───────────────────────────────────────────────────────

    /// Returns Some(config) on 200 OK with a full static configuration.
    #[tokio::test]
    async fn test_get_bmc_config_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "ip_address_source": "static",
                    "ip_address": "192.168.1.100",
                    "netmask": "255.255.255.0",
                    "gateway": "192.168.1.1",
                    "username": "admin",
                    "password": "secret"
                }"#,
            )
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let config = result.unwrap().unwrap();
        assert_eq!(config.ip_address_source, "static");
        assert_eq!(config.ip_address, Some("192.168.1.100".parse().unwrap()));
        assert_eq!(config.netmask, Some("255.255.255.0".parse().unwrap()));
        assert_eq!(config.gateway, Some("192.168.1.1".parse().unwrap()));
        assert_eq!(config.username, Some("admin".to_string()));
        assert_eq!(config.password, Some("secret".to_string()));
    }

    /// Returns None on 404.
    #[tokio::test]
    async fn test_get_bmc_config_not_found() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(404)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Returns an error on 500.
    #[tokio::test]
    async fn test_get_bmc_config_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(500)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    /// DHCP config with no static IP fields deserializes correctly.
    #[tokio::test]
    async fn test_get_bmc_config_dhcp() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ip_address_source":"dhcp","username":"admin","password":"secret"}"#)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        let config = result.unwrap().unwrap();
        assert_eq!(config.ip_address_source, "dhcp");
        assert_eq!(config.ip_address, None);
    }

    /// Missing `ip_address_source` defaults to `"static"`.
    #[tokio::test]
    async fn test_get_bmc_config_default_ip_source() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"ip_address":"10.0.0.1","netmask":"255.255.255.0","gateway":"10.0.0.254"}"#,
            )
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        let config = result.unwrap().unwrap();
        assert_eq!(config.ip_address_source, "static");
    }

    /// Malformed JSON body returns an error.
    #[tokio::test]
    async fn test_get_bmc_config_malformed_json() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"invalid": json}"#)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
    }

    // ── get_disk_layout ──────────────────────────────────────────────────────

    /// Returns a deserialized layout on 200 OK.
    #[tokio::test]
    async fn test_get_disk_layout_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/disk_layout")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"disks":[{"device":"/dev/disk/by-path/pci-0000:00:1f.2-ata-1","partition_table":"gpt","partitions":[{"label":"root","size":"rest","filesystem":"ext4","mount_point":"/"}]}]}"#)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_disk_layout("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let layout = result.unwrap();
        assert_eq!(layout.disks.len(), 1);
        assert_eq!(
            layout.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    /// Returns an error on 400 (e.g. no role assigned).
    #[tokio::test]
    async fn test_get_disk_layout_no_role() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/disk_layout")
            .with_status(400)
            .with_body("Device has no role assigned")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_disk_layout("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("400"));
    }

    /// Returns an error on 404.
    #[tokio::test]
    async fn test_get_disk_layout_not_found() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/devices/test-uuid/disk_layout")
            .with_status(404)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_disk_layout("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("404"));
    }

    // ── poll ─────────────────────────────────────────────────────────────────

    /// Returns Some(PollResponse) on 200 OK with a valid action payload.
    #[tokio::test]
    async fn test_poll_returns_action_on_200() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/poll")
            .match_query(mockito::Matcher::UrlEncoded(
                "uuid".to_string(),
                "test-uuid".to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type":"action","payload":{"type":"discover_hardware"}}"#)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.poll("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let PollResponse::Action { payload } = result.unwrap().unwrap();
        assert_eq!(payload, PollAction::DiscoverHardware);
    }

    /// Returns None on 204 No Content.
    #[tokio::test]
    async fn test_poll_returns_none_on_204() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/poll")
            .match_query(mockito::Matcher::UrlEncoded(
                "uuid".to_string(),
                "test-uuid".to_string(),
            ))
            .with_status(204)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.poll("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Returns an error on 500.
    #[tokio::test]
    async fn test_poll_returns_error_on_500() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/poll")
            .match_query(mockito::Matcher::UrlEncoded(
                "uuid".to_string(),
                "test-uuid".to_string(),
            ))
            .with_status(500)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.poll("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    /// Deserializes all known PollAction variants correctly.
    #[tokio::test]
    async fn test_poll_deserializes_all_action_variants() {
        let test_cases = [
            (
                r#"{"type":"action","payload":{"type":"discover_hardware"}}"#,
                PollAction::DiscoverHardware,
            ),
            (
                r#"{"type":"action","payload":{"type":"configure_bmc"}}"#,
                PollAction::ConfigureBmc,
            ),
            (
                r#"{"type":"action","payload":{"type":"partition_disks"}}"#,
                PollAction::PartitionDisks,
            ),
            (
                r#"{"type":"action","payload":{"type":"reboot_device"}}"#,
                PollAction::RebootDevice,
            ),
            (
                r#"{"type":"action","payload":{"type":"install_os"}}"#,
                PollAction::InstallOs,
            ),
        ];

        let mut server = mockito::Server::new_async().await;
        let client = CncClient::new(&server.url());

        for (body, expected_action) in test_cases {
            let mock = server
                .mock("GET", "/cnc/poll")
                .match_query(mockito::Matcher::Any)
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(body)
                .create_async()
                .await;

            let result = client.poll("any-uuid").await;
            mock.assert_async().await;
            assert!(result.is_ok(), "Expected Ok for body: {}", body);
            let PollResponse::Action { payload } = result.unwrap().unwrap();
            assert_eq!(payload, expected_action, "Mismatch for body: {}", body);
        }
    }

    // ── get_ipxe_script ──────────────────────────────────────────────────────

    /// Returns the script body on 200 OK.
    #[tokio::test]
    async fn test_get_ipxe_script_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/ipxe")
            .with_status(200)
            .with_body("#!ipxe\nchain http://example.com/boot.ipxe?uuid=${uuid}")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_ipxe_script(None, None).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("#!ipxe"));
    }

    /// A non-2xx response returns an error containing the status code.
    #[tokio::test]
    async fn test_get_ipxe_script_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/ipxe")
            .with_status(500)
            .with_body("internal error")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_ipxe_script(None, None).await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    // ── get_agent_image ──────────────────────────────────────────────────────

    /// Returns raw bytes on 200 OK.
    #[tokio::test]
    async fn test_get_agent_image_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/agent-images/vmlinuz")
            .with_status(200)
            .with_body(b"FAKE_KERNEL_BYTES".as_slice())
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_agent_image("vmlinuz").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"FAKE_KERNEL_BYTES");
    }

    /// A 404 response returns an error.
    #[tokio::test]
    async fn test_get_agent_image_not_found() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cnc/agent-images/missing.img")
            .with_status(404)
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let result = client.get_agent_image("missing.img").await;

        mock.assert_async().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("404"));
    }
}
