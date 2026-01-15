use anyhow::{Context, Result, anyhow};
use serde_json::{Map, Value};

use crate::ConnectionConfig;
use crate::output::Output;

pub struct HttpClient {
    client: reqwest::Client,
    base_url: String,
}

impl HttpClient {
    pub fn new(conn: &ConnectionConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: format!("http://{}:{}", conn.host, conn.http_port),
        }
    }

    pub async fn get_ipxe_script(&self, uuid: Option<&str>, mac: Option<&str>, output: &Output) -> Result<String> {
        let mut url = format!("{}/cnc/ipxe", self.base_url);
        let mut params = Vec::new();

        if let Some(u) = uuid {
            params.push(format!("uuid={}", u));
        }
        if let Some(m) = mac {
            params.push(format!("mac={}", m));
        }

        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }

        output.info(&format!("GET {}", url));

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch iPXE script from {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {} from {}: {}", status, url, body));
        }

        let body = response.text().await?;

        output.info(&format!("Response: {} bytes", body.len()));
        if output.is_verbose() {
            for line in body.lines().take(10) {
                output.info(&format!("  {}", line));
            }
            if body.lines().count() > 10 {
                output.info("  ...");
            }
        }

        Ok(body)
    }

    pub async fn get_agent_image(&self, filename: &str, output: &Output) -> Result<Vec<u8>> {
        let url = format!("{}/cnc/agent-images/{}", self.base_url, filename);

        output.info(&format!("GET {}", url));

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch agent image from {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {} from {}: {}", status, url, body));
        }

        let bytes = response.bytes().await?.to_vec();

        output.info(&format!("Received {} bytes", bytes.len()));

        Ok(bytes)
    }

    pub async fn update_attributes(
        &self,
        uuid: &str,
        attributes: Map<String, Value>,
        output: &Output,
    ) -> Result<()> {
        let url = format!("{}/cnc/update_attributes", self.base_url);

        let body = serde_json::json!({
            "uuid": uuid,
            "attributes": attributes
        });

        output.info(&format!("POST {}", url));
        if output.is_verbose() {
            output.info(&format!("  uuid: {}", uuid));
            output.info(&format!("  attributes: {} fields", attributes.len()));
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to post attributes to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {} from {}: {}", status, url, body));
        }

        output.info("Attributes updated successfully");

        Ok(())
    }

    pub async fn action_success(&self, uuid: &str, output: &Output) -> Result<()> {
        let url = format!("{}/cnc/action_success", self.base_url);

        let body = serde_json::json!({
            "uuid": uuid
        });

        output.info(&format!("POST {}", url));

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to post action_success to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {} from {}: {}", status, url, body));
        }

        output.info("Action success reported");

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn action_failed(
        &self,
        uuid: &str,
        error_message: &str,
        output: &Output,
    ) -> Result<()> {
        let url = format!("{}/cnc/action_failed", self.base_url);

        let body = serde_json::json!({
            "uuid": uuid,
            "error_message": error_message
        });

        output.info(&format!("POST {}", url));

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to post action_failed to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {} from {}: {}", status, url, body));
        }

        output.info("Action failure reported");

        Ok(())
    }
}

pub fn parse_ipxe_script(script: &str) -> IpxeScript {
    let mut result = IpxeScript::default();

    for line in script.lines() {
        let line = line.trim();

        if line.starts_with("kernel ") {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                result.kernel_url = Some(parts[1].to_string());
            }
            if parts.len() >= 3 {
                result.cmdline = Some(parts[2].to_string());
            }
        } else if line.starts_with("initrd ") {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                result.initrd_url = Some(parts[1].to_string());
            }
        } else if line.starts_with("chain ") {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                result.chain_url = Some(parts[1].to_string());
            }
        } else if line.starts_with("sanboot ") {
            result.is_local_boot = true;
        }
    }

    result
}

#[derive(Debug, Default)]
pub struct IpxeScript {
    pub kernel_url: Option<String>,
    pub initrd_url: Option<String>,
    pub cmdline: Option<String>,
    pub chain_url: Option<String>,
    pub is_local_boot: bool,
}
