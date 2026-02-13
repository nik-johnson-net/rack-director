use std::net::Ipv4Addr;

use anyhow::Result;
use serde_json::{Value, json};

use crate::ConnectionConfig;
use crate::hardware_profiles::HardwareConfig;
use crate::http::HttpClient;
use crate::output::Output;
use crate::server::ServerState;
use common::device_attributes::{CpuInfo, MemoryInfo};

pub async fn run(conn: &ConnectionConfig, state: &ServerState, output: &Output) -> Result<()> {
    output.step("AGENT SIMULATION");
    output.detail("UUID", &state.uuid);
    output.detail(
        "Manufacturer",
        state.hardware.manufacturer.as_deref().unwrap_or("Unknown"),
    );
    output.detail(
        "Product",
        state.hardware.product_name.as_deref().unwrap_or("Unknown"),
    );
    output.info(&format!(
        "Reporting {} network interfaces",
        state.mac_addresses.len()
    ));

    let http = HttpClient::new(conn);

    output.info("Building hardware attributes...");
    let attributes = build_attributes(&state.hardware, &state.server_name, state);

    output.info(&format!("Uploading {} attributes...", attributes.len()));
    http.update_attributes(&state.uuid, attributes, output)
        .await?;

    output.info("Reporting action success...");
    http.action_success(&state.uuid, output).await?;

    output.success("Agent simulation complete");

    Ok(())
}

fn build_attributes(
    hardware: &HardwareConfig,
    server_name: &str,
    state: &ServerState,
) -> serde_json::Map<String, serde_json::Value> {
    let mut attrs = serde_json::Map::new();

    if let Some(manufacturer) = &hardware.manufacturer {
        attrs.insert("manufacturer".to_string(), json!(manufacturer));
    }

    if let Some(product_name) = &hardware.product_name {
        attrs.insert("product_name".to_string(), json!(product_name));
    }

    let serial = hardware
        .serial_number
        .clone()
        .unwrap_or_else(|| generate_serial(server_name));
    attrs.insert("serial_number".to_string(), json!(serial));

    if let Some(bios_version) = &hardware.bios_version {
        attrs.insert("bios_version".to_string(), json!(bios_version));
    }

    if let Some(bios_vendor) = &hardware.bios_vendor {
        attrs.insert("bios_vendor".to_string(), json!(bios_vendor));
    }

    let processor_count = hardware.processor_count.unwrap_or(1);
    let cores = hardware.cores_per_processor.unwrap_or(8);
    let threads = hardware.threads_per_core.unwrap_or(2);

    // Build CPUs using CpuInfo type from common crate
    let cpus: Vec<CpuInfo> = (0..processor_count)
        .map(|i| CpuInfo {
            designation: Some(format!("CPU{}", i)),
            manufacturer: hardware.cpu_manufacturer.clone(),
            model: hardware.cpu_model.clone(),
            cores: Some(cores as u32),
            threads: Some((cores * threads) as u32),
            speed_mhz: Some(2400),
        })
        .collect();
    attrs.insert("cpus".to_string(), json!(cpus));

    let dimm_count = hardware.memory_dimm_count.unwrap_or(4);
    let dimm_size = hardware.memory_dimm_size_mb.unwrap_or(8192);
    let dimm_speed = hardware.memory_speed_mhz.unwrap_or(2400);

    // Build memory using MemoryInfo type from common crate
    let memory: Vec<MemoryInfo> = (0..dimm_count)
        .map(|_i| MemoryInfo {
            size_mb: Some(dimm_size),
            speed_mhz: Some(dimm_speed as u32),
            manufacturer: Some("Samsung".to_string()),
            part_number: Some("M393A2K40DB3-CWE".to_string()),
        })
        .collect();
    attrs.insert("memory".to_string(), json!(memory));

    let total_memory = hardware
        .total_memory_mb
        .unwrap_or((dimm_count as u64) * (dimm_size as u64));
    attrs.insert("total_memory_mb".to_string(), json!(total_memory));

    // Build disks array from hardware profile
    let disks: Vec<_> = hardware
        .disks
        .iter()
        .enumerate()
        .map(|(idx, disk)| {
            json!({
                "name": disk.name,
                "size": disk.size_gb,
                "disk_type": disk.disk_type,
                "model": disk.model,
                "serial": format!("SIM{:08X}", simple_hash(format!("{}-{}", server_name, idx).as_bytes())),
                "path": generate_disk_path(&disk.name, &disk.disk_type, idx),
            })
        })
        .collect();
    attrs.insert("disks".to_string(), json!(disks));

    // Build network_interfaces array
    let network_interfaces: Vec<_> = state
        .mac_addresses
        .iter()
        .enumerate()
        .map(|(idx, mac)| {
            let mac_string = crate::server::format_mac(mac);
            let ip_address = state
                .allocated_ips
                .get(idx)
                .and_then(|ip| ip.as_ref().map(|i| i.to_string()));

            // Get NIC speed from hardware profile if available
            let speed_mbps = hardware.nics.get(idx).map(|nic| nic.speed_mbps);

            json!({
                "interface_name": format!("eth{}", idx),
                "mac_address": mac_string,
                "ip_address": ip_address,
                "speed_mbps": speed_mbps,
            })
        })
        .collect();
    attrs.insert("network_interfaces".to_string(), json!(network_interfaces));

    // Also set legacy mac_address field for backward compatibility
    if let Some(primary_mac) = state.mac_addresses.first() {
        attrs.insert(
            "mac_address".to_string(),
            json!(crate::server::format_mac(primary_mac)),
        );
    }

    // Detect BMC
    if let Some(bmc) = build_bmc(hardware, state) {
        attrs.insert("bmc".to_owned(), bmc);
    }

    attrs
}

fn build_bmc(_hardware: &HardwareConfig, state: &ServerState) -> Option<Value> {
    if let Some(bmc) = &state.bmc {
        let map = json!({
            "mac_address": crate::server::format_mac(&bmc.mac_address),
            "ip_address_source": bmc.ip_source,
            "ip_address": bmc.allocated_ip.as_ref().unwrap_or(&Ipv4Addr::new(0, 0, 0 ,0)),
            "ip_netmask": bmc.netmask.as_ref().unwrap_or(&Ipv4Addr::new(0, 0, 0, 0)),
            "ip_gateway": bmc.gateway.as_ref().unwrap_or(&Ipv4Addr::new(0, 0, 0, 0)),
        });
        return Some(map);
    }
    None
}

fn generate_serial(seed: &str) -> String {
    let hash = simple_hash(seed.as_bytes());
    format!("SN{:08X}", hash)
}

fn simple_hash(data: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(*byte as u32);
    }
    hash
}

/// Generate a stable /dev/disk/by-path/ style path for a disk
///
/// This mimics real disk paths by creating bus-based paths that vary by disk type:
/// - NVMe: `/dev/disk/by-path/pci-0000:XX:00.0-nvme-1`
/// - SATA/SAS SSD/HDD: `/dev/disk/by-path/pci-0000:XX:1f.2-ata-N`
fn generate_disk_path(
    _name: &str,
    disk_type: &common::device_attributes::DiskType,
    index: usize,
) -> String {
    use common::device_attributes::DiskType;

    match disk_type {
        DiskType::Nvme => {
            // NVMe devices typically appear on their own PCIe bus
            // Format: pci-0000:01:00.0-nvme-1 (where 01 is bus, nvme-1 is namespace)
            let bus = index + 1; // Start at bus 1 for first NVMe
            format!("/dev/disk/by-path/pci-0000:{:02x}:00.0-nvme-1", bus)
        }
        DiskType::Ssd | DiskType::Hdd => {
            // SATA/SAS devices typically appear on a storage controller
            // Format: pci-0000:00:1f.2-ata-1 (where 00:1f.2 is typical SATA controller)
            // ATA port number increments per device
            let ata_port = index + 1;
            format!("/dev/disk/by-path/pci-0000:00:1f.2-ata-{}", ata_port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Architecture, ResolvedBMC, ResolvedServer};
    use crate::hardware_profiles::HardwareConfig;

    #[test]
    fn test_build_attributes_single_nic() {
        let config = ResolvedServer {
            name: "test".to_string(),
            macs: vec![[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware: HardwareConfig::default(),
            bmc: Some(ResolvedBMC {
                mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0xFF],
                source: "DHCP".to_string(),
                ip_address: None,
                ip_network: None,
                gateway: None,
            }),
        };
        let mut state = ServerState::new("test", &config);
        state.allocated_ips[0] = Some("192.168.1.100".parse().unwrap());

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check network_interfaces
        let network_interfaces = attrs.get("network_interfaces").unwrap().as_array().unwrap();
        assert_eq!(network_interfaces.len(), 1);

        let nic0 = &network_interfaces[0];
        assert_eq!(nic0["interface_name"], "eth0");
        assert_eq!(nic0["mac_address"], "52:54:00:12:34:56");
        assert_eq!(nic0["ip_address"], "192.168.1.100");

        // Check legacy mac_address field
        assert_eq!(attrs.get("mac_address").unwrap(), "52:54:00:12:34:56");
    }

    #[test]
    fn test_build_attributes_multiple_nics() {
        let config = ResolvedServer {
            name: "test".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware: HardwareConfig::default(),
            bmc: Some(ResolvedBMC {
                mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0xFF],
                source: "DHCP".to_string(),
                ip_address: None,
                ip_network: None,
                gateway: None,
            }),
        };
        let mut state = ServerState::new("test", &config);
        state.allocated_ips[0] = Some("192.168.1.100".parse().unwrap());
        state.allocated_ips[1] = Some("192.168.1.101".parse().unwrap());

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check network_interfaces
        let network_interfaces = attrs.get("network_interfaces").unwrap().as_array().unwrap();
        assert_eq!(network_interfaces.len(), 2);

        let nic0 = &network_interfaces[0];
        assert_eq!(nic0["interface_name"], "eth0");
        assert_eq!(nic0["mac_address"], "52:54:00:12:34:56");
        assert_eq!(nic0["ip_address"], "192.168.1.100");

        let nic1 = &network_interfaces[1];
        assert_eq!(nic1["interface_name"], "eth1");
        assert_eq!(nic1["mac_address"], "52:54:00:12:34:57");
        assert_eq!(nic1["ip_address"], "192.168.1.101");

        // Check legacy mac_address field (should be first NIC)
        assert_eq!(attrs.get("mac_address").unwrap(), "52:54:00:12:34:56");
    }

    #[test]
    fn test_build_attributes_no_ips() {
        let config = ResolvedServer {
            name: "test".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware: HardwareConfig::default(),
            bmc: Some(ResolvedBMC {
                mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0xFF],
                source: "DHCP".to_string(),
                ip_address: None,
                ip_network: None,
                gateway: None,
            }),
        };
        let state = ServerState::new("test", &config);

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check network_interfaces
        let network_interfaces = attrs.get("network_interfaces").unwrap().as_array().unwrap();
        assert_eq!(network_interfaces.len(), 2);

        // IPs should be null when not allocated
        let nic0 = &network_interfaces[0];
        assert!(nic0["ip_address"].is_null());

        let nic1 = &network_interfaces[1];
        assert!(nic1["ip_address"].is_null());
    }

    #[test]
    fn test_build_attributes_with_disks() {
        use crate::hardware_profiles::{DiskConfig, NicConfig};
        use common::device_attributes::DiskType;

        let mut hardware = HardwareConfig::default();
        hardware.disks = vec![
            DiskConfig {
                name: "nvme0n1".to_string(),
                size_gb: 960,
                disk_type: DiskType::Nvme,
                model: "Samsung 970 EVO".to_string(),
            },
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Ssd,
                model: "Samsung 860 EVO".to_string(),
            },
        ];
        hardware.nics = vec![NicConfig { speed_mbps: 10000 }];

        let config = ResolvedServer {
            name: "test-disks".to_string(),
            macs: vec![[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware,
            bmc: None,
        };
        let state = ServerState::new("test-disks", &config);

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check disks
        let disks = attrs.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 2);

        // Verify NVMe disk
        let nvme = &disks[0];
        assert_eq!(nvme["name"], "nvme0n1");
        assert_eq!(nvme["size"], 960);
        assert_eq!(nvme["disk_type"], "nvme");
        assert_eq!(nvme["model"], "Samsung 970 EVO");
        assert!(nvme["serial"].as_str().unwrap().starts_with("SIM"));
        assert_eq!(nvme["path"], "/dev/disk/by-path/pci-0000:01:00.0-nvme-1");

        // Verify SSD disk
        let ssd = &disks[1];
        assert_eq!(ssd["name"], "sda");
        assert_eq!(ssd["size"], 1920);
        assert_eq!(ssd["disk_type"], "ssd");
        assert_eq!(ssd["model"], "Samsung 860 EVO");
        assert!(ssd["serial"].as_str().unwrap().starts_with("SIM"));
        assert_eq!(ssd["path"], "/dev/disk/by-path/pci-0000:00:1f.2-ata-2");
    }

    #[test]
    fn test_build_attributes_with_nic_speeds() {
        use crate::hardware_profiles::NicConfig;

        let mut hardware = HardwareConfig::default();
        hardware.nics = vec![
            NicConfig { speed_mbps: 10000 },
            NicConfig { speed_mbps: 25000 },
        ];

        let config = ResolvedServer {
            name: "test-nics".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware,
            bmc: None,
        };
        let state = ServerState::new("test-nics", &config);

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check network_interfaces
        let network_interfaces = attrs.get("network_interfaces").unwrap().as_array().unwrap();
        assert_eq!(network_interfaces.len(), 2);

        // Verify first NIC speed
        let nic0 = &network_interfaces[0];
        assert_eq!(nic0["interface_name"], "eth0");
        assert_eq!(nic0["speed_mbps"], 10000);

        // Verify second NIC speed
        let nic1 = &network_interfaces[1];
        assert_eq!(nic1["interface_name"], "eth1");
        assert_eq!(nic1["speed_mbps"], 25000);
    }

    #[test]
    fn test_build_attributes_nic_count_mismatch() {
        use crate::hardware_profiles::NicConfig;

        // Hardware profile with only 1 NIC config
        let mut hardware = HardwareConfig::default();
        hardware.nics = vec![NicConfig { speed_mbps: 10000 }];

        // Server with 2 MAC addresses
        let config = ResolvedServer {
            name: "test".to_string(),
            macs: vec![
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57],
            ],
            uuid: "test-uuid".to_string(),
            architecture: Architecture::X64Uefi,
            hardware,
            bmc: None,
        };
        let state = ServerState::new("test", &config);

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        // Check network_interfaces
        let network_interfaces = attrs.get("network_interfaces").unwrap().as_array().unwrap();
        assert_eq!(network_interfaces.len(), 2);

        // First NIC should have speed from profile
        let nic0 = &network_interfaces[0];
        assert_eq!(nic0["speed_mbps"], 10000);

        // Second NIC should have null speed (no config)
        let nic1 = &network_interfaces[1];
        assert!(nic1["speed_mbps"].is_null());
    }

    #[test]
    fn test_generate_disk_path_nvme() {
        use common::device_attributes::DiskType;

        let path = generate_disk_path("nvme0n1", &DiskType::Nvme, 0);
        assert_eq!(path, "/dev/disk/by-path/pci-0000:01:00.0-nvme-1");

        let path = generate_disk_path("nvme1n1", &DiskType::Nvme, 1);
        assert_eq!(path, "/dev/disk/by-path/pci-0000:02:00.0-nvme-1");
    }

    #[test]
    fn test_generate_disk_path_sata() {
        use common::device_attributes::DiskType;

        let path = generate_disk_path("sda", &DiskType::Ssd, 0);
        assert_eq!(path, "/dev/disk/by-path/pci-0000:00:1f.2-ata-1");

        let path = generate_disk_path("sdb", &DiskType::Hdd, 1);
        assert_eq!(path, "/dev/disk/by-path/pci-0000:00:1f.2-ata-2");
    }

    #[test]
    fn test_dell_r640_hardware_profile() {
        use crate::hardware_profiles;

        let profile = hardware_profiles::dell_r640();

        // Verify disk configuration
        assert_eq!(profile.disks.len(), 6);
        assert_eq!(profile.disks[0].name, "nvme0n1");
        assert_eq!(profile.disks[0].size_gb, 960);
        assert_eq!(profile.disks[1].name, "nvme1n1");
        assert_eq!(profile.disks[2].name, "sda");

        // Verify NIC configuration
        assert_eq!(profile.nics.len(), 2);
        assert_eq!(profile.nics[0].speed_mbps, 10000);
        assert_eq!(profile.nics[1].speed_mbps, 10000);
    }
}
