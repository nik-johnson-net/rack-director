use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::hardware_profiles::{self, HardwareConfig};
use crate::output::Output;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    // Support both old single MAC and new multiple MACs for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_addresses: Option<Vec<String>>,
    pub uuid: String,
    pub architecture: String,
    #[serde(default)]
    pub hardware_profile: Option<String>,
    #[serde(default)]
    pub hardware: Option<HardwareConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86Bios,
    X64Uefi,
    Arm64Uefi,
}

impl Architecture {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "x86-bios" | "x86_bios" | "bios" => Ok(Self::X86Bios),
            "x64-uefi" | "x64_uefi" | "uefi" | "x86-64" => Ok(Self::X64Uefi),
            "arm64-uefi" | "arm64_uefi" | "arm64" | "aarch64" => Ok(Self::Arm64Uefi),
            _ => Err(anyhow!(
                "Unknown architecture: {}. Use x86-bios, x64-uefi, or arm64-uefi",
                s
            )),
        }
    }

    pub fn dhcp_option_93(&self) -> u16 {
        match self {
            Self::X86Bios => 0,
            Self::X64Uefi => 7,
            Self::Arm64Uefi => 11,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::X86Bios => "x86-bios",
            Self::X64Uefi => "x64-uefi",
            Self::Arm64Uefi => "arm64-uefi",
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Config::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;

        Ok(())
    }

    pub fn get_server(&self, name: &str) -> Result<ResolvedServer> {
        let server = self
            .servers
            .get(name)
            .ok_or_else(|| anyhow!("Server '{}' not found in config", name))?;

        let macs = resolve_macs(server, name)?;
        let uuid = resolve_uuid(&server.uuid, name)?;
        let architecture = Architecture::from_str(&server.architecture)?;

        let base_hardware = server
            .hardware_profile
            .as_ref()
            .map(|p| hardware_profiles::get_profile(p))
            .transpose()?
            .unwrap_or_else(hardware_profiles::generic);

        let hardware = if let Some(overrides) = &server.hardware {
            merge_hardware(&base_hardware, overrides)
        } else {
            base_hardware
        };

        Ok(ResolvedServer {
            name: name.to_string(),
            macs,
            uuid,
            architecture,
            hardware,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub name: String,
    pub macs: Vec<[u8; 6]>,
    pub uuid: String,
    pub architecture: Architecture,
    pub hardware: HardwareConfig,
}

/// Resolve MAC addresses from ServerConfig
/// Supports:
/// - mac_addresses: ["52:54:00:12:34:56", "52:54:00:12:34:57"] (multiple)
/// - mac_address: "52:54:00:12:34:56" (single, legacy) - generates 2 sequential MACs
/// - mac_address: "auto" (auto-generate 2 MACs from server name)
fn resolve_macs(server: &ServerConfig, server_name: &str) -> Result<Vec<[u8; 6]>> {
    // Priority 1: Use mac_addresses if provided
    if let Some(mac_strings) = &server.mac_addresses {
        return mac_strings.iter().map(|s| parse_mac(s)).collect();
    }

    // Priority 2: Use mac_address (legacy) - generate 2 sequential MACs
    if let Some(mac_string) = &server.mac_address {
        let first_mac = if mac_string == "auto" {
            generate_mac(server_name)
        } else {
            parse_mac(mac_string)?
        };
        let second_mac = increment_mac(first_mac);
        return Ok(vec![first_mac, second_mac]);
    }

    // Fallback: Auto-generate 2 MACs
    let first_mac = generate_mac(server_name);
    let second_mac = increment_mac(first_mac);
    Ok(vec![first_mac, second_mac])
}

fn resolve_uuid(value: &str, server_name: &str) -> Result<String> {
    if value == "auto" {
        Ok(generate_uuid(server_name))
    } else {
        Ok(value.to_string())
    }
}

fn generate_mac(seed: &str) -> [u8; 6] {
    let hash = simple_hash(seed.as_bytes());
    let mut mac = [0u8; 6];
    mac[0] = 0x52; // Locally administered, unicast
    mac[1] = 0x54;
    mac[2] = 0x00;
    mac[3] = ((hash >> 16) & 0xFF) as u8;
    mac[4] = ((hash >> 8) & 0xFF) as u8;
    mac[5] = (hash & 0xFF) as u8;
    mac
}

/// Increment a MAC address by 1
fn increment_mac(mac: [u8; 6]) -> [u8; 6] {
    let mut new_mac = mac;
    // Increment last byte with overflow handling
    if new_mac[5] == 255 {
        new_mac[5] = 0;
        if new_mac[4] == 255 {
            new_mac[4] = 0;
            new_mac[3] = new_mac[3].wrapping_add(1);
        } else {
            new_mac[4] = new_mac[4].wrapping_add(1);
        }
    } else {
        new_mac[5] = new_mac[5].wrapping_add(1);
    }
    new_mac
}

fn generate_uuid(seed: &str) -> String {
    let hash1 = simple_hash(seed.as_bytes());
    let hash2 = simple_hash(&[seed.as_bytes(), b"uuid"].concat());
    let hash3 = simple_hash(&[seed.as_bytes(), b"uuid2"].concat());
    let hash4 = simple_hash(&[seed.as_bytes(), b"uuid3"].concat());

    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        hash1,
        (hash2 & 0xFFFF),
        (hash2 >> 16) & 0xFFF,
        0x8000 | (hash3 & 0x3FFF),
        ((hash4 as u64) << 16) | (hash1 as u64 & 0xFFFF)
    )
}

fn simple_hash(data: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(*byte as u32);
    }
    hash
}

fn parse_mac(s: &str) -> Result<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return Err(anyhow!("Invalid MAC address format: {}", s));
    }

    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16)
            .with_context(|| format!("Invalid MAC address byte: {}", part))?;
    }

    Ok(mac)
}

fn merge_hardware(base: &HardwareConfig, overrides: &HardwareConfig) -> HardwareConfig {
    HardwareConfig {
        manufacturer: overrides
            .manufacturer
            .clone()
            .or_else(|| base.manufacturer.clone()),
        product_name: overrides
            .product_name
            .clone()
            .or_else(|| base.product_name.clone()),
        serial_number: overrides
            .serial_number
            .clone()
            .or_else(|| base.serial_number.clone()),
        bios_vendor: overrides
            .bios_vendor
            .clone()
            .or_else(|| base.bios_vendor.clone()),
        bios_version: overrides
            .bios_version
            .clone()
            .or_else(|| base.bios_version.clone()),
        total_memory_mb: overrides.total_memory_mb.or(base.total_memory_mb),
        processor_count: overrides.processor_count.or(base.processor_count),
        cores_per_processor: overrides.cores_per_processor.or(base.cores_per_processor),
        threads_per_core: overrides.threads_per_core.or(base.threads_per_core),
        memory_dimm_count: overrides.memory_dimm_count.or(base.memory_dimm_count),
        memory_dimm_size_mb: overrides.memory_dimm_size_mb.or(base.memory_dimm_size_mb),
        memory_speed_mhz: overrides.memory_speed_mhz.or(base.memory_speed_mhz),
    }
}

pub fn create_server(
    config_path: &Path,
    name: &str,
    mac: &str,
    uuid: &str,
    arch: &str,
    profile: Option<&str>,
) -> Result<()> {
    Architecture::from_str(arch)?;

    let mut config = Config::load(config_path)?;

    if config.servers.contains_key(name) {
        return Err(anyhow!("Server '{}' already exists", name));
    }

    config.servers.insert(
        name.to_string(),
        ServerConfig {
            mac_address: Some(mac.to_string()),
            mac_addresses: None,
            uuid: uuid.to_string(),
            architecture: arch.to_string(),
            hardware_profile: profile.map(String::from),
            hardware: None,
        },
    );

    config.save(config_path)?;

    Ok(())
}

pub fn remove_server(config_path: &Path, name: &str) -> Result<()> {
    let mut config = Config::load(config_path)?;

    if config.servers.remove(name).is_none() {
        return Err(anyhow!("Server '{}' not found", name));
    }

    config.save(config_path)?;

    Ok(())
}

pub fn list_servers(config: &Config, output: &Output) {
    if config.servers.is_empty() {
        output.info("No servers configured");
        return;
    }

    println!("{:<20} {:<18} {:<12}", "NAME", "MAC(s)", "ARCH");
    println!("{:-<20} {:-<18} {:-<12}", "", "", "");

    for (name, server) in &config.servers {
        // Display first MAC or "auto"
        let mac_display = server
            .mac_addresses
            .as_ref()
            .and_then(|v| v.first())
            .map(|s| s.as_str())
            .or(server.mac_address.as_deref())
            .unwrap_or("auto");

        let mac_count = server.mac_addresses.as_ref().map(|v| v.len()).unwrap_or(2); // Default to 2 NICs

        let mac_str = if mac_count > 1 {
            format!("{} (+{})", mac_display, mac_count - 1)
        } else {
            mac_display.to_string()
        };

        println!("{:<20} {:<18} {:<12}", name, mac_str, server.architecture);
    }
}

pub fn show_server(config: &Config, name: &str, output: &Output) -> Result<()> {
    let resolved = config.get_server(name)?;

    output.step(&format!("Server: {}", resolved.name));

    // Display all MAC addresses
    if resolved.macs.len() == 1 {
        output.detail("MAC Address", &crate::server::format_mac(&resolved.macs[0]));
    } else {
        for (idx, mac) in resolved.macs.iter().enumerate() {
            output.detail(
                &format!("MAC Address (eth{})", idx),
                &crate::server::format_mac(mac),
            );
        }
    }

    output.detail("UUID", &resolved.uuid);
    output.detail("Architecture", resolved.architecture.as_str());

    if let Some(manufacturer) = &resolved.hardware.manufacturer {
        output.detail("Manufacturer", manufacturer);
    }
    if let Some(product) = &resolved.hardware.product_name {
        output.detail("Product", product);
    }
    if let Some(memory) = resolved.hardware.total_memory_mb {
        output.detail("Total Memory", &format!("{} MB", memory));
    }
    if let Some(procs) = resolved.hardware.processor_count {
        let cores = resolved.hardware.cores_per_processor.unwrap_or(1);
        let threads = resolved.hardware.threads_per_core.unwrap_or(1);
        output.detail(
            "Processors",
            &format!("{}x ({} cores, {} threads each)", procs, cores, threads),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mac() {
        let mac = parse_mac("52:54:00:12:34:56").unwrap();
        assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    }

    #[test]
    fn test_generate_mac_deterministic() {
        let mac1 = generate_mac("test-server");
        let mac2 = generate_mac("test-server");
        assert_eq!(mac1, mac2);
    }

    #[test]
    fn test_generate_uuid_deterministic() {
        let uuid1 = generate_uuid("test-server");
        let uuid2 = generate_uuid("test-server");
        assert_eq!(uuid1, uuid2);
    }

    #[test]
    fn test_architecture_from_str() {
        assert_eq!(
            Architecture::from_str("x86-bios").unwrap(),
            Architecture::X86Bios
        );
        assert_eq!(
            Architecture::from_str("x64-uefi").unwrap(),
            Architecture::X64Uefi
        );
        assert_eq!(
            Architecture::from_str("arm64-uefi").unwrap(),
            Architecture::Arm64Uefi
        );
        assert!(Architecture::from_str("invalid").is_err());
    }

    #[test]
    fn test_increment_mac() {
        // Normal increment
        let mac1 = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let mac2 = increment_mac(mac1);
        assert_eq!(mac2, [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]);

        // Overflow last byte
        let mac3 = [0x52, 0x54, 0x00, 0x12, 0x34, 0xFF];
        let mac4 = increment_mac(mac3);
        assert_eq!(mac4, [0x52, 0x54, 0x00, 0x12, 0x35, 0x00]);

        // Overflow two bytes
        let mac5 = [0x52, 0x54, 0x00, 0x12, 0xFF, 0xFF];
        let mac6 = increment_mac(mac5);
        assert_eq!(mac6, [0x52, 0x54, 0x00, 0x13, 0x00, 0x00]);
    }

    #[test]
    fn test_resolve_macs_explicit_multiple() {
        let server = ServerConfig {
            mac_address: None,
            mac_addresses: Some(vec![
                "52:54:00:12:34:56".to_string(),
                "52:54:00:12:34:57".to_string(),
                "52:54:00:12:34:58".to_string(),
            ]),
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 3);
        assert_eq!(macs[0], [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(macs[1], [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]);
        assert_eq!(macs[2], [0x52, 0x54, 0x00, 0x12, 0x34, 0x58]);
    }

    #[test]
    fn test_resolve_macs_legacy_single() {
        let server = ServerConfig {
            mac_address: Some("52:54:00:12:34:56".to_string()),
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        assert_eq!(macs[0], [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(macs[1], [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]);
    }

    #[test]
    fn test_resolve_macs_auto() {
        let server = ServerConfig {
            mac_address: Some("auto".to_string()),
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        // Should be deterministic
        let expected_first = generate_mac("test-server");
        assert_eq!(macs[0], expected_first);
        assert_eq!(macs[1], increment_mac(expected_first));
    }

    #[test]
    fn test_resolve_macs_neither_field() {
        let server = ServerConfig {
            mac_address: None,
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        let expected_first = generate_mac("test-server");
        assert_eq!(macs[0], expected_first);
        assert_eq!(macs[1], increment_mac(expected_first));
    }
}
