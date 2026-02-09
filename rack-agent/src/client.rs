use anyhow::Result;
use serde::{Deserialize, Serialize};

pub struct RackDirector {
    client: reqwest::Client,
    url: String,
}

#[derive(Serialize)]
struct UpdateAttributesPayload {
    uuid: String,
    attributes: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize)]
struct ActionStatusPayload {
    uuid: String,
}

#[derive(Serialize)]
struct ActionFailedPayload {
    uuid: String,
    error_message: String,
}

fn default_ip_source() -> String {
    "static".to_string()
}

#[derive(Debug, Deserialize)]
pub struct BmcConfig {
    #[serde(default = "default_ip_source")]
    pub ip_address_source: String, // "static" or "dhcp"
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub netmask: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl RackDirector {
    pub fn new(url: &str) -> RackDirector {
        RackDirector {
            client: reqwest::Client::new(),
            url: url.to_string(),
        }
    }

    pub async fn update_attributes(
        &self,
        uuid: &str,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let payload = UpdateAttributesPayload {
            uuid: uuid.to_string(),
            attributes,
        };

        let response = self
            .client
            .post(format!("{}/update_attributes", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to update attributes: {}", response.status());
        }

        Ok(())
    }

    pub async fn action_success(&self, uuid: &str) -> Result<()> {
        let payload = ActionStatusPayload {
            uuid: uuid.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/action_success", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to report action success: {}", response.status());
        }

        Ok(())
    }

    pub async fn action_failed(&self, uuid: &str, error_message: &str) -> Result<()> {
        let payload = ActionFailedPayload {
            uuid: uuid.to_string(),
            error_message: error_message.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/action_failed", self.url))
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to report action failure: {}", response.status());
        }

        Ok(())
    }

    /// Fetch BMC configuration for a device from rack-director
    ///
    /// Returns:
    /// - Ok(Some(config)) if configuration exists (200 OK)
    /// - Ok(None) if no configuration is set for this device (404 Not Found)
    /// - Err(...) for network errors, server errors, or unexpected status codes
    pub async fn get_bmc_config(&self, uuid: &str) -> Result<Option<BmcConfig>> {
        let response = self
            .client
            .get(format!("{}/devices/{}/bmc_config", self.url, uuid))
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let config = response.json::<BmcConfig>().await?;
                Ok(Some(config))
            }
            reqwest::StatusCode::NOT_FOUND => {
                // No BMC configuration is set for this device - this is not an error
                Ok(None)
            }
            status => {
                anyhow::bail!("Failed to fetch BMC config: {}", status)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test get_bmc_config returns Some(config) on 200 OK
    #[tokio::test]
    async fn test_get_bmc_config_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
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

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let config_option = result.unwrap();
        assert!(config_option.is_some());

        let config = config_option.unwrap();
        assert_eq!(config.ip_address_source, "static");
        assert_eq!(config.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(config.netmask, Some("255.255.255.0".to_string()));
        assert_eq!(config.gateway, Some("192.168.1.1".to_string()));
        assert_eq!(config.username, Some("admin".to_string()));
        assert_eq!(config.password, Some("secret".to_string()));
    }

    /// Test get_bmc_config returns None on 404 Not Found
    #[tokio::test]
    async fn test_get_bmc_config_not_found() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(404)
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Test get_bmc_config returns error on 500 Internal Server Error
    #[tokio::test]
    async fn test_get_bmc_config_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(500)
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("500"));
    }

    /// Test get_bmc_config returns error on 401 Unauthorized
    #[tokio::test]
    async fn test_get_bmc_config_unauthorized() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(401)
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("401"));
    }

    /// Test get_bmc_config with DHCP configuration (minimal fields)
    #[tokio::test]
    async fn test_get_bmc_config_dhcp() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "ip_address_source": "dhcp",
                    "username": "admin",
                    "password": "secret"
                }"#,
            )
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let config_option = result.unwrap();
        assert!(config_option.is_some());

        let config = config_option.unwrap();
        assert_eq!(config.ip_address_source, "dhcp");
        assert_eq!(config.ip_address, None);
        assert_eq!(config.netmask, None);
        assert_eq!(config.gateway, None);
        assert_eq!(config.username, Some("admin".to_string()));
        assert_eq!(config.password, Some("secret".to_string()));
    }

    /// Test get_bmc_config with default ip_address_source
    #[tokio::test]
    async fn test_get_bmc_config_default_ip_source() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "ip_address": "192.168.1.100",
                    "netmask": "255.255.255.0",
                    "gateway": "192.168.1.1"
                }"#,
            )
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let config_option = result.unwrap();
        assert!(config_option.is_some());

        let config = config_option.unwrap();
        // Should default to "static" when not specified
        assert_eq!(config.ip_address_source, "static");
        assert_eq!(config.ip_address, Some("192.168.1.100".to_string()));
    }

    /// Test get_bmc_config handles malformed JSON response
    #[tokio::test]
    async fn test_get_bmc_config_malformed_json() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/devices/test-uuid/bmc_config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"invalid": json}"#)
            .create_async()
            .await;

        let client = RackDirector::new(&server.url());
        let result = client.get_bmc_config("test-uuid").await;

        mock.assert_async().await;
        assert!(result.is_err());
    }
}
