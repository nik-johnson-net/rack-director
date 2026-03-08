use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

use crate::e2e::config::{PlatformSpec, RoleSpec};
use crate::output::Output;
use crate::vm::qemu::{DirectorVmConfig, QemuProcess, director_vm_args, find_available_tcp_port};

/// A running director VM with HTTP API access.
pub struct DirectorVm {
    pub host_http_port: u16,
    _process: QemuProcess,
    client: Client,
}

impl DirectorVm {
    /// Start the director VM and wait for it to become ready.
    pub async fn start(
        kernel: PathBuf,
        initramfs: PathBuf,
        net_port: u16,
        agent_net_port: u16,
        memory_mb: u32,
        serial_log: Option<PathBuf>,
    ) -> Result<Self> {
        let host_http_port = find_available_tcp_port(30000, 39999)?;
        let pcap_log = serial_log.as_ref().map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("director");
            p.parent()
                .unwrap_or(std::path::Path::new("."))
                .join(format!("{}-net0.pcap", stem))
        });
        let config = DirectorVmConfig {
            kernel,
            initramfs,
            net_port,
            agent_net_port,
            host_http_port,
            memory_mb,
            serial_log,
            pcap_log,
        };
        let args = director_vm_args(&config);
        let process = QemuProcess::spawn("director", &args)?;
        let client = Client::new();

        let vm = Self {
            host_http_port,
            _process: process,
            client,
        };
        // Allow up to 20 minutes for TCG (software emulation) which can be slow on Windows
        vm.wait_ready(Duration::from_secs(1200)).await?;
        Ok(vm)
    }

    /// Poll GET /ui/devices until the server responds successfully.
    async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let url = format!("{}/ui/devices", self.host_url());
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "Director VM did not become ready within {:?}",
                    timeout
                ));
            }
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }

    /// Create a DHCP network for the rack subnet (10.0.0.0/24) and a pool (10.0.0.10-10.0.0.100).
    /// This must be called before starting the agent VM so the agent can get an IP via DHCP.
    pub async fn create_rack_dhcp_network(&self) -> Result<()> {
        // Create the network
        let url = format!("{}/ui/dhcp/networks", self.host_url());
        let body = json!({
            "name": "rack",
            "subnet": "10.0.0.0/24",
            "gateway": "10.0.0.1",
            "dns_servers": ["8.8.8.8"],
            "lease_duration": 3600,
            "enable_autodiscovery": true
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to create DHCP network")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create DHCP network failed ({}): {}", status, text));
        }

        let value: Value = resp
            .json()
            .await
            .context("Failed to parse DHCP network response")?;
        let network_id = value["id"]
            .as_i64()
            .ok_or_else(|| anyhow!("No id in DHCP network response"))?;

        // Create a pool within the network
        let pool_url = format!("{}/ui/dhcp/networks/{}/pools", self.host_url(), network_id);
        let pool_body = json!({
            "name": "rack-pool",
            "range_start": "10.0.0.10",
            "range_end": "10.0.0.100"
        });
        let pool_resp = self
            .client
            .post(&pool_url)
            .json(&pool_body)
            .send()
            .await
            .context("Failed to create DHCP pool")?;

        if !pool_resp.status().is_success() {
            let status = pool_resp.status();
            let text = pool_resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create DHCP pool failed ({}): {}", status, text));
        }

        Ok(())
    }

    /// Create a stub operating system record for use in roles.
    pub async fn create_stub_os(&self) -> Result<i64> {
        let url = format!("{}/ui/operating_systems", self.host_url());
        let body = json!({
            "name": "stub-os",
            "version": "1.0",
            "kernel_cmdline": "console=ttyS0 quiet"
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to create stub OS")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create OS failed ({}): {}", status, text));
        }

        let value: Value = resp.json().await.context("Failed to parse OS response")?;
        value["id"]
            .as_i64()
            .ok_or_else(|| anyhow!("No id in OS response"))
    }

    /// Create a platform from a spec.
    pub async fn create_platform(&self, spec: &PlatformSpec) -> Result<i64> {
        let url = format!("{}/ui/platforms", self.host_url());
        let body = json!({
            "name": spec.name,
            "attributes": spec.attributes
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to create platform")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create platform failed ({}): {}", status, text));
        }

        let value: Value = resp
            .json()
            .await
            .context("Failed to parse platform response")?;
        value["id"]
            .as_i64()
            .ok_or_else(|| anyhow!("No id in platform response"))
    }

    /// Create a role from a spec.
    pub async fn create_role(&self, spec: &RoleSpec, os_id: i64) -> Result<i64> {
        let url = format!("{}/ui/roles", self.host_url());
        let disk_layout_json =
            serde_json::to_value(&spec.disk_layout).context("Failed to serialize disk layout")?;
        let body = json!({
            "name": spec.name,
            "os_id": os_id,
            "disk_layout": disk_layout_json
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to create role")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create role failed ({}): {}", status, text));
        }

        let value: Value = resp.json().await.context("Failed to parse role response")?;
        value["id"]
            .as_i64()
            .ok_or_else(|| anyhow!("No id in role response"))
    }

    /// Wait for a device to appear in the devices list (polls every 2s).
    pub async fn wait_for_device(&self, timeout: Duration) -> Result<String> {
        let url = format!("{}/ui/devices", self.host_url());
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!("No device appeared within {:?}", timeout));
            }
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(value) = resp.json::<Value>().await {
                        if let Some(devices) = value["devices"].as_array() {
                            if let Some(device) = devices.first() {
                                if let Some(uuid) = device["uuid"].as_str() {
                                    return Ok(uuid.to_string());
                                }
                            }
                        }
                    }
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }

    /// Assign a platform to a device.
    pub async fn assign_platform(&self, uuid: &str, platform_id: i64) -> Result<()> {
        let url = format!("{}/ui/devices/{}/platform", self.host_url(), uuid);
        let body = json!({ "platform_id": platform_id });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to assign platform")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Assign platform failed ({}): {}", status, text));
        }
        Ok(())
    }

    /// Assign a role to a device.
    pub async fn assign_role(&self, uuid: &str, role_id: i64) -> Result<()> {
        let url = format!("{}/ui/devices/{}/role", self.host_url(), uuid);
        let body = json!({ "role_id": role_id });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to assign role")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Assign role failed ({}): {}", status, text));
        }
        Ok(())
    }

    /// Wait until the device has no active lifecycle transition (polls every 2s).
    pub async fn wait_for_idle(&self, uuid: &str, timeout: Duration) -> Result<()> {
        let url = format!("{}/ui/devices/{}/transitions/active", self.host_url(), uuid);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "Device {} still has an active transition after {:?}",
                    uuid,
                    timeout
                ));
            }
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(value) = resp.json::<Value>().await {
                        if value.is_null() {
                            return Ok(());
                        }
                    }
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }

    /// Start a lifecycle transition for a device.
    pub async fn start_transition(&self, uuid: &str, to_state: &str) -> Result<()> {
        let url = format!(
            "{}/ui/devices/{}/lifecycle/transition",
            self.host_url(),
            uuid
        );
        let body = json!({ "to_state": to_state });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to start transition")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Start transition failed ({}): {}", status, text));
        }
        Ok(())
    }

    /// Get the current lifecycle state of a device.
    pub async fn get_lifecycle_state(&self, uuid: &str) -> Result<String> {
        let url = format!("{}/ui/devices/{}", self.host_url(), uuid);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to get device")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Get device failed ({}): {}", status, text));
        }

        let value: Value = resp
            .json()
            .await
            .context("Failed to parse device response")?;
        value["lifecycle"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("No lifecycle in device response"))
    }

    /// Set up a Rocky Linux OS architecture: create the arch row, upload kernel,
    /// initramfs, and kickstart install script.
    pub async fn setup_rocky_linux_os(
        &self,
        os_id: i64,
        kernel: &Path,
        initramfs: &Path,
        kickstart: &Path,
        output: &Output,
    ) -> Result<()> {
        let arch_url = format!(
            "{}/ui/operating_systems/{}/architectures",
            self.host_url(),
            os_id
        );
        let body = json!({
            "architecture": "x86-64",
            "cmdline_args": "inst.ks={{install_script_url}}"
        });
        let resp = self
            .client
            .post(&arch_url)
            .json(&body)
            .send()
            .await
            .context("Failed to create OS architecture")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Create OS architecture failed ({}): {}",
                status,
                text
            ));
        }

        output.info("Uploading Rocky Linux kernel...");
        self.upload_os_file(os_id, "x86-64", "kernel", kernel)
            .await?;

        output.info("Uploading Rocky Linux initramfs...");
        self.upload_os_file(os_id, "x86-64", "initramfs", initramfs)
            .await?;

        output.info("Uploading Rocky Linux kickstart...");
        self.upload_os_file(os_id, "x86-64", "install_script", kickstart)
            .await?;

        Ok(())
    }

    /// Upload a file to an OS architecture component endpoint.
    async fn upload_os_file(
        &self,
        os_id: i64,
        arch: &str,
        component: &str,
        path: &Path,
    ) -> Result<()> {
        let url = format!(
            "{}/ui/operating_systems/{}/architectures/{}/{}",
            self.host_url(),
            os_id,
            arch,
            component
        );
        let file_bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(file_bytes)
            .send()
            .await
            .with_context(|| format!("Failed to upload {}", component))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Upload {} failed ({}): {}",
                component,
                status,
                text
            ));
        }
        Ok(())
    }

    /// The host-accessible URL of rack-director (via hostfwd).
    pub fn host_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.host_http_port)
    }
}
