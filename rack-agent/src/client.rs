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
    pub async fn get_bmc_config(&self, uuid: &str) -> Result<BmcConfig> {
        let response = self
            .client
            .get(format!("{}/devices/{}/bmc_config", self.url, uuid))
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch BMC config: {}", response.status());
        }

        let config = response.json::<BmcConfig>().await?;
        Ok(config)
    }
}
