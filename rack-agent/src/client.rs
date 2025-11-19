use anyhow::Result;
use serde::Serialize;

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
}
