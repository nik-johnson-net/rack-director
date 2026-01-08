use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use crate::config::{Architecture, ResolvedServer};
use crate::hardware_profiles::HardwareConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub server_name: String,
    pub mac_address: [u8; 6],
    pub uuid: String,
    pub architecture: String,
    pub hardware: HardwareConfig,

    #[serde(default)]
    pub allocated_ip: Option<Ipv4Addr>,
    #[serde(default)]
    pub tftp_server: Option<Ipv4Addr>,
    #[serde(default)]
    pub bootfile: Option<String>,
    #[serde(default)]
    pub boot_script_url: Option<String>,
    #[serde(default)]
    pub xid: Option<u32>,
}

impl ServerState {
    pub fn load_or_create(name: &str, config: &ResolvedServer) -> Result<Self> {
        let path = state_file_path(name);

        if path.exists() {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read state file: {}", path.display()))?;

            let mut state: ServerState = serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse state file: {}", path.display()))?;

            state.mac_address = config.mac;
            state.uuid = config.uuid.clone();
            state.architecture = config.architecture.as_str().to_string();
            state.hardware = config.hardware.clone();

            Ok(state)
        } else {
            Ok(Self::new(name, config))
        }
    }

    pub fn new(name: &str, config: &ResolvedServer) -> Self {
        Self {
            server_name: name.to_string(),
            mac_address: config.mac,
            uuid: config.uuid.clone(),
            architecture: config.architecture.as_str().to_string(),
            hardware: config.hardware.clone(),
            allocated_ip: None,
            tftp_server: None,
            bootfile: None,
            boot_script_url: None,
            xid: None,
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = state_file_path(&self.server_name);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;

        Ok(())
    }

    pub fn clear_state(&mut self) {
        self.allocated_ip = None;
        self.tftp_server = None;
        self.bootfile = None;
        self.boot_script_url = None;
        self.xid = None;
    }

    pub fn architecture(&self) -> Result<Architecture> {
        Architecture::from_str(&self.architecture)
    }

    pub fn mac_string(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.mac_address[0],
            self.mac_address[1],
            self.mac_address[2],
            self.mac_address[3],
            self.mac_address[4],
            self.mac_address[5]
        )
    }
}

fn state_file_path(name: &str) -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rack-simulator")
        .join(format!("{}.state.json", name))
}

#[allow(dead_code)]
pub fn delete_state(name: &str) -> Result<()> {
    let path = state_file_path(name);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
