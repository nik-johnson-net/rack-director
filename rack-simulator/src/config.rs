use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;

use crate::hardware_profiles::{self, HardwareConfig};
use crate::output::Output;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcConfig {
    pub source: String,
    pub ip_address: Option<Ipv4Addr>,
    pub netmask: Option<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    pub mac_address: String,
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
    #[serde(default)]
    pub bmc: Option<BmcConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86Bios,
    X64Uefi,
    Arm64Uefi,
    X64UefiHttp,
}

impl Architecture {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "x86-bios" | "x86_bios" | "bios" => Ok(Self::X86Bios),
            "x64-uefi" | "x64_uefi" | "uefi" | "x86-64" => Ok(Self::X64Uefi),
            "arm64-uefi" | "arm64_uefi" | "arm64" | "aarch64" => Ok(Self::Arm64Uefi),
            "x64-uefi-http" | "x64_uefi_http" | "http" => Ok(Self::X64UefiHttp),
            _ => Err(anyhow!(
                "Unknown architecture: {}. Use x86-bios, x64-uefi, arm64-uefi, or x64-uefi-http",
                s
            )),
        }
    }

    pub fn dhcp_option_93(&self) -> u16 {
        match self {
            Self::X86Bios => 0,
            Self::X64Uefi => 7,
            Self::Arm64Uefi => 11,
            Self::X64UefiHttp => 15,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::X86Bios => "x86-bios",
            Self::X64Uefi => "x64-uefi",
            Self::Arm64Uefi => "arm64-uefi",
            Self::X64UefiHttp => "x64-uefi-http",
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

        let bmc = if let Some(bmc_config) = &server.bmc {
            let mac = resolve_mac_bmc(bmc_config, name)?;
            Some(ResolvedBMC {
                source: bmc_config.source.clone(),
                ip_address: bmc_config.ip_address,
                ip_network: bmc_config.netmask,
                gateway: bmc_config.gateway,
                mac,
            })
        } else {
            None
        };

        Ok(ResolvedServer {
            name: name.to_string(),
            macs,
            uuid,
            architecture,
            hardware,
            bmc,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedBMC {
    pub source: String,
    pub ip_address: Option<Ipv4Addr>,
    pub ip_network: Option<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    pub mac: [u8; 6],
}

#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub name: String,
    pub macs: Vec<[u8; 6]>,
    pub uuid: String,
    pub architecture: Architecture,
    pub hardware: HardwareConfig,
    pub bmc: Option<ResolvedBMC>,
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
        if mac_string == "auto" {
            // Auto-generate with NIC index to ensure uniqueness
            let macs: Vec<[u8; 6]> = (0..2)
                .map(|idx| generate_mac(&format!("{}-nic{}", server_name, idx)))
                .collect();
            return Ok(macs);
        } else {
            let first_mac = parse_mac(mac_string)?;
            let second_mac = increment_mac(first_mac);
            return Ok(vec![first_mac, second_mac]);
        }
    }

    // Fallback: Auto-generate 2 MACs with NIC index to ensure uniqueness
    let macs: Vec<[u8; 6]> = (0..2)
        .map(|idx| generate_mac(&format!("{}-nic{}", server_name, idx)))
        .collect();
    Ok(macs)
}

fn resolve_mac_bmc(bmc: &BmcConfig, server_name: &str) -> Result<[u8; 6]> {
    if bmc.mac_address == "auto" {
        Ok(generate_mac_bmc(server_name))
    } else {
        parse_mac(&bmc.mac_address)
    }
}

fn generate_mac_bmc(server_name: &str) -> [u8; 6] {
    generate_mac(&format!("{}-bmc", server_name))
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
        cpu_manufacturer: overrides
            .cpu_manufacturer
            .clone()
            .or_else(|| base.cpu_manufacturer.clone()),
        cpu_model: overrides
            .cpu_model
            .clone()
            .or_else(|| base.cpu_model.clone()),
        // Use override disks if present, otherwise use base disks
        disks: if !overrides.disks.is_empty() {
            overrides.disks.clone()
        } else {
            base.disks.clone()
        },
        // Use override NICs if present, otherwise use base NICs
        nics: if !overrides.nics.is_empty() {
            overrides.nics.clone()
        } else {
            base.nics.clone()
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_server(
    config_path: &Path,
    name: &str,
    mac: &str,
    uuid: &str,
    arch: &str,
    profile: Option<&str>,
    bmc_mac: Option<&str>,
    bmc_source: &str,
    bmc_ip_address: Option<&Ipv4Addr>,
    bmc_netmask: Option<&Ipv4Addr>,
    bmc_gateway: Option<&Ipv4Addr>,
) -> Result<()> {
    Architecture::from_str(arch)?;

    let mut config = Config::load(config_path)?;

    if config.servers.contains_key(name) {
        return Err(anyhow!("Server '{}' already exists", name));
    }

    let bmc = if let Some(bmc_mac) = bmc_mac {
        if bmc_source == "Static" && (bmc_ip_address.is_none() || bmc_netmask.is_none()) {
            return Err(anyhow!(
                "bmc-ip-address and bmc-netmask must be defined if bmc-source is 'Static'"
            ));
        }

        Some(BmcConfig {
            source: bmc_source.to_string(),
            ip_address: bmc_ip_address.cloned(),
            netmask: bmc_netmask.cloned(),
            mac_address: bmc_mac.to_string(),
            gateway: bmc_gateway.cloned(),
        })
    } else {
        None
    };

    config.servers.insert(
        name.to_string(),
        ServerConfig {
            mac_address: Some(mac.to_string()),
            mac_addresses: None,
            uuid: uuid.to_string(),
            architecture: arch.to_string(),
            hardware_profile: profile.map(String::from),
            hardware: None,
            bmc,
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

    if let Some(bmc) = resolved.bmc {
        output.detail("BMC MAC", &crate::server::format_mac(&bmc.mac));
        output.detail("BMC IP Source", &bmc.source);
        output.detail(
            "BMC IP Address",
            &bmc.ip_address
                .as_ref()
                .unwrap_or(&Ipv4Addr::new(0, 0, 0, 0))
                .to_string(),
        );
        output.detail(
            "BMC IP Netmask",
            &bmc.ip_network
                .as_ref()
                .unwrap_or(&Ipv4Addr::new(0, 0, 0, 0))
                .to_string(),
        );
        output.detail(
            "BMC IP Gateway",
            &bmc.gateway
                .as_ref()
                .unwrap_or(&Ipv4Addr::new(0, 0, 0, 0))
                .to_string(),
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
        assert_eq!(
            Architecture::from_str("x64-uefi-http").unwrap(),
            Architecture::X64UefiHttp
        );
        assert_eq!(
            Architecture::from_str("x64_uefi_http").unwrap(),
            Architecture::X64UefiHttp
        );
        assert_eq!(
            Architecture::from_str("http").unwrap(),
            Architecture::X64UefiHttp
        );
        assert!(Architecture::from_str("invalid").is_err());
    }

    #[test]
    fn test_architecture_dhcp_option_93() {
        assert_eq!(Architecture::X86Bios.dhcp_option_93(), 0);
        assert_eq!(Architecture::X64Uefi.dhcp_option_93(), 7);
        assert_eq!(Architecture::Arm64Uefi.dhcp_option_93(), 11);
        assert_eq!(Architecture::X64UefiHttp.dhcp_option_93(), 15);
    }

    #[test]
    fn test_architecture_as_str() {
        assert_eq!(Architecture::X86Bios.as_str(), "x86-bios");
        assert_eq!(Architecture::X64Uefi.as_str(), "x64-uefi");
        assert_eq!(Architecture::Arm64Uefi.as_str(), "arm64-uefi");
        assert_eq!(Architecture::X64UefiHttp.as_str(), "x64-uefi-http");
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
            bmc: None,
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
            bmc: None,
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
            bmc: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        // Should be deterministic and use NIC index
        let expected_first = generate_mac("test-server-nic0");
        let expected_second = generate_mac("test-server-nic1");
        assert_eq!(macs[0], expected_first);
        assert_eq!(macs[1], expected_second);
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
            bmc: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        // After fix, should use NIC index in seed
        let expected_first = generate_mac("test-server-nic0");
        let expected_second = generate_mac("test-server-nic1");
        assert_eq!(macs[0], expected_first);
        assert_eq!(macs[1], expected_second);
    }

    #[test]
    fn test_different_servers_same_name_get_different_macs() {
        // This simulates the scenario where two servers are created with the same name
        // In practice, this might happen if servers are created and destroyed, or in
        // testing scenarios where name reuse occurs
        let server_config = ServerConfig {
            mac_address: None,
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
            bmc: None,
        };

        // Get MACs for two "instances" with the same name
        let macs1 = resolve_macs(&server_config, "server1").unwrap();
        let macs2 = resolve_macs(&server_config, "server1").unwrap();

        // They should be identical (deterministic) when using the same name
        assert_eq!(macs1, macs2);

        // But servers with different names should get different MACs
        let macs3 = resolve_macs(&server_config, "server2").unwrap();
        assert_ne!(macs1[0], macs3[0]);
        assert_ne!(macs1[1], macs3[1]);
    }

    #[test]
    fn test_each_nic_gets_unique_mac_within_server() {
        let server = ServerConfig {
            mac_address: None,
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
            bmc: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);

        // Each NIC should have a different MAC address
        assert_ne!(macs[0], macs[1]);
    }

    #[test]
    fn test_mac_generation_is_deterministic_with_nic_index() {
        // Same server name + same NIC index should always produce same MAC
        let mac1_nic0 = generate_mac("test-server-nic0");
        let mac2_nic0 = generate_mac("test-server-nic0");
        assert_eq!(mac1_nic0, mac2_nic0);

        let mac1_nic1 = generate_mac("test-server-nic1");
        let mac2_nic1 = generate_mac("test-server-nic1");
        assert_eq!(mac1_nic1, mac2_nic1);

        // But different NIC indices should produce different MACs
        assert_ne!(mac1_nic0, mac1_nic1);
    }

    #[test]
    fn test_resolve_macs_auto_uses_nic_index() {
        let server = ServerConfig {
            mac_address: Some("auto".to_string()),
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
            bmc: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);

        // Should use NIC index in seed
        let expected_first = generate_mac("test-server-nic0");
        let expected_second = generate_mac("test-server-nic1");
        assert_eq!(macs[0], expected_first);
        assert_eq!(macs[1], expected_second);

        // MACs should be different
        assert_ne!(macs[0], macs[1]);
    }

    #[test]
    fn test_resolve_macs_explicit_mac_still_increments() {
        // When an explicit MAC is provided (not "auto"), we should still increment
        // for backward compatibility with existing configs
        let server = ServerConfig {
            mac_address: Some("52:54:00:12:34:56".to_string()),
            mac_addresses: None,
            uuid: "test-uuid".to_string(),
            architecture: "x64-uefi".to_string(),
            hardware_profile: None,
            hardware: None,
            bmc: None,
        };

        let macs = resolve_macs(&server, "test-server").unwrap();
        assert_eq!(macs.len(), 2);
        assert_eq!(macs[0], [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(macs[1], [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]);
    }
}
