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
    // Support both old single MAC and new multiple MACs for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<[u8; 6]>,
    pub mac_addresses: Vec<[u8; 6]>,
    pub uuid: String,
    pub architecture: String,
    pub hardware: HardwareConfig,

    #[serde(default)]
    pub allocated_ip: Option<Ipv4Addr>,
    #[serde(default)]
    pub allocated_ips: Vec<Option<Ipv4Addr>>,
    #[serde(default)]
    pub tftp_server: Option<Ipv4Addr>,
    #[serde(default)]
    pub bootfile: Option<String>,
    #[serde(default)]
    pub boot_script_url: Option<String>,
    #[serde(default)]
    pub xid: Option<u32>,
    #[serde(default)]
    pub current_nic_index: usize,
}

impl ServerState {
    pub fn load_or_create(name: &str, config: &ResolvedServer) -> Result<Self> {
        let path = state_file_path(name);

        if path.exists() {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read state file: {}", path.display()))?;

            let mut state: ServerState = serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse state file: {}", path.display()))?;

            // Migrate old single MAC format to new multiple MACs format
            if state.mac_addresses.is_empty()
                && let Some(old_mac) = state.mac_address
            {
                state.mac_addresses = vec![old_mac];
                state.mac_address = None;
            }

            // Update with config
            state.mac_addresses = config.macs.clone();
            state.uuid = config.uuid.clone();
            state.architecture = config.architecture.as_str().to_string();
            state.hardware = config.hardware.clone();

            // Ensure allocated_ips matches mac_addresses length
            if state.allocated_ips.len() != state.mac_addresses.len() {
                state.allocated_ips.resize(state.mac_addresses.len(), None);
            }

            Ok(state)
        } else {
            Ok(Self::new(name, config))
        }
    }

    pub fn new(name: &str, config: &ResolvedServer) -> Self {
        let mac_count = config.macs.len();
        Self {
            server_name: name.to_string(),
            mac_address: None, // Deprecated field
            mac_addresses: config.macs.clone(),
            uuid: config.uuid.clone(),
            architecture: config.architecture.as_str().to_string(),
            hardware: config.hardware.clone(),
            allocated_ip: None,
            allocated_ips: vec![None; mac_count],
            tftp_server: None,
            bootfile: None,
            boot_script_url: None,
            xid: None,
            current_nic_index: 0,
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
        self.allocated_ips = vec![None; self.mac_addresses.len()];
        self.tftp_server = None;
        self.bootfile = None;
        self.boot_script_url = None;
        self.xid = None;
        self.current_nic_index = 0;
    }

    pub fn architecture(&self) -> Result<Architecture> {
        Architecture::from_str(&self.architecture)
    }

    /// Format MAC address as a string.
    /// If index is provided, format that specific MAC.
    /// If index is None, format all MACs comma-separated.
    pub fn mac_string(&self, index: Option<usize>) -> String {
        match index {
            Some(idx) => {
                if idx < self.mac_addresses.len() {
                    format_mac(&self.mac_addresses[idx])
                } else {
                    String::from("invalid-index")
                }
            }
            None => self
                .mac_addresses
                .iter()
                .map(format_mac)
                .collect::<Vec<_>>()
                .join(", "),
        }
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

/// Format a MAC address array as a colon-separated string
pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_mac() {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        assert_eq!(format_mac(&mac), "52:54:00:12:34:56");
    }

    #[test]
    fn test_mac_string_single() {
        let config = crate::config::ResolvedServer {
            name: "test".to_string(),
            macs: vec![[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]],
            uuid: "test-uuid".to_string(),
            architecture: crate::config::Architecture::X64Uefi,
            hardware: crate::hardware_profiles::HardwareConfig::default(),
        };
        let state = ServerState::new("test", &config);
        assert_eq!(state.mac_string(Some(0)), "52:54:00:12:34:56");
        assert_eq!(state.mac_string(None), "52:54:00:12:34:56");
    }

    #[test]
    fn test_mac_string_multiple() {
        let config = crate::config::ResolvedServer {
            name: "test".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: crate::config::Architecture::X64Uefi,
            hardware: crate::hardware_profiles::HardwareConfig::default(),
        };
        let state = ServerState::new("test", &config);
        assert_eq!(state.mac_string(Some(0)), "52:54:00:12:34:56");
        assert_eq!(state.mac_string(Some(1)), "52:54:00:12:34:57");
        assert_eq!(
            state.mac_string(None),
            "52:54:00:12:34:56, 52:54:00:12:34:57"
        );
    }

    #[test]
    fn test_clear_state_resets_all_ips() {
        let config = crate::config::ResolvedServer {
            name: "test".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: crate::config::Architecture::X64Uefi,
            hardware: crate::hardware_profiles::HardwareConfig::default(),
        };
        let mut state = ServerState::new("test", &config);
        state.allocated_ips[0] = Some("192.168.1.100".parse().unwrap());
        state.allocated_ips[1] = Some("192.168.1.101".parse().unwrap());
        state.clear_state();
        assert_eq!(state.allocated_ips.len(), 2);
        assert!(state.allocated_ips[0].is_none());
        assert!(state.allocated_ips[1].is_none());
    }
}
