use std::net::Ipv4Addr;

use anyhow::Result;

use crate::hardware_profiles::HardwareConfig;
use crate::output::Output;
use crate::server::ServerState;
use common::cnc::CncClient;
use common::device_attributes::{
    BmcInfo, CpuInfo, DeviceAttributes, DiskInfo, MemoryInfo, NetworkInterface,
};

pub async fn run(cnc: &CncClient, state: &ServerState, output: &Output) -> Result<()> {
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

    output.info("Building hardware attributes...");
    let attributes = build_attributes(&state.hardware, &state.server_name, state);

    output.info("Uploading hardware attributes...");
    cnc.update_attributes(&state.uuid, &attributes).await?;

    output.info("Reporting action success...");
    cnc.action_success(&state.uuid).await?;

    output.success("Agent simulation complete");

    Ok(())
}

#[allow(clippy::field_reassign_with_default)]
fn build_attributes(
    hardware: &HardwareConfig,
    server_name: &str,
    state: &ServerState,
) -> DeviceAttributes {
    let mut attributes = DeviceAttributes::default();

    attributes.manufacturer = hardware.manufacturer.clone();
    attributes.product_name = hardware.product_name.clone();
    attributes.serial_number = Some(
        hardware
            .serial_number
            .clone()
            .unwrap_or_else(|| generate_serial(server_name)),
    );
    attributes.bios_version = hardware.bios_version.clone();
    attributes.bios_vendor = hardware.bios_vendor.clone();

    let processor_count = hardware.processor_count.unwrap_or(1);
    let cores = hardware.cores_per_processor.unwrap_or(8);
    let threads = hardware.threads_per_core.unwrap_or(2);

    attributes.cpus = (0..processor_count)
        .map(|i| CpuInfo {
            designation: Some(format!("CPU{}", i)),
            manufacturer: hardware.cpu_manufacturer.clone(),
            model: hardware.cpu_model.clone(),
            cores: Some(cores as u32),
            threads: Some((cores * threads) as u32),
            speed_mhz: Some(2400),
        })
        .collect();

    let dimm_count = hardware.memory_dimm_count.unwrap_or(4);
    let dimm_size = hardware.memory_dimm_size_mb.unwrap_or(8192);
    let dimm_speed = hardware.memory_speed_mhz.unwrap_or(2400);

    attributes.memory = (0..dimm_count)
        .map(|_| MemoryInfo {
            size_mb: Some(dimm_size),
            speed_mhz: Some(dimm_speed as u32),
            manufacturer: Some("Samsung".to_string()),
            part_number: Some("M393A2K40DB3-CWE".to_string()),
        })
        .collect();

    let total_memory = hardware
        .total_memory_mb
        .unwrap_or((dimm_count as u64) * (dimm_size as u64));
    attributes.extra.insert(
        "total_memory_mb".to_string(),
        serde_json::json!(total_memory),
    );

    attributes.disks = hardware
        .disks
        .iter()
        .enumerate()
        .map(|(idx, disk)| DiskInfo {
            name: disk.name.clone(),
            size: Some(disk.size_gb),
            disk_type: Some(disk.disk_type),
            model: Some(disk.model.clone()),
            serial: Some(format!(
                "SIM{:08X}",
                simple_hash(format!("{}-{}", server_name, idx).as_bytes())
            )),
            path: Some(generate_disk_path(&disk.name, &disk.disk_type, idx)),
            ..Default::default()
        })
        .collect();

    attributes.network_interfaces = state
        .mac_addresses
        .iter()
        .enumerate()
        .map(|(idx, mac)| {
            let mac_string = crate::server::format_mac(mac);
            let ip_address = state
                .allocated_ips
                .get(idx)
                .and_then(|ip| ip.as_ref().map(|i| i.to_string()));
            let speed_mbps = hardware.nics.get(idx).map(|nic| nic.speed_mbps);

            NetworkInterface {
                interface_name: format!("eth{}", idx),
                mac_address: mac_string,
                ip_address,
                speed_mbps,
                ..Default::default()
            }
        })
        .collect();

    // Legacy mac_address field for backward compatibility
    if let Some(primary_mac) = state.mac_addresses.first() {
        attributes.mac_address = Some(crate::server::format_mac(primary_mac));
    }

    attributes.bmc = build_bmc(state);

    attributes
}

fn build_bmc(state: &ServerState) -> Option<BmcInfo> {
    state.bmc.as_ref().map(|bmc| BmcInfo {
        mac_address: crate::server::format_mac(&bmc.mac_address),
        ip_address: bmc
            .allocated_ip
            .as_ref()
            .map(|ip| ip.to_string())
            .or_else(|| Some(Ipv4Addr::new(0, 0, 0, 0).to_string())),
        ip_address_source: Some(bmc.ip_source.clone()),
    })
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
            let bus = index + 1;
            format!("/dev/disk/by-path/pci-0000:{:02x}:00.0-nvme-1", bus)
        }
        DiskType::Ssd | DiskType::Hdd => {
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

        assert_eq!(attrs.network_interfaces.len(), 1);
        let nic0 = &attrs.network_interfaces[0];
        assert_eq!(nic0.interface_name, "eth0");
        assert_eq!(nic0.mac_address, "52:54:00:12:34:56");
        assert_eq!(nic0.ip_address, Some("192.168.1.100".to_string()));

        assert_eq!(attrs.mac_address, Some("52:54:00:12:34:56".to_string()));
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
            bmc: None,
        };
        let mut state = ServerState::new("test", &config);
        state.allocated_ips[0] = Some("192.168.1.100".parse().unwrap());
        state.allocated_ips[1] = Some("192.168.1.101".parse().unwrap());

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        assert_eq!(attrs.network_interfaces.len(), 2);
        let nic0 = &attrs.network_interfaces[0];
        assert_eq!(nic0.interface_name, "eth0");
        assert_eq!(nic0.mac_address, "52:54:00:12:34:56");
        assert_eq!(nic0.ip_address, Some("192.168.1.100".to_string()));

        let nic1 = &attrs.network_interfaces[1];
        assert_eq!(nic1.interface_name, "eth1");
        assert_eq!(nic1.mac_address, "52:54:00:12:34:57");
        assert_eq!(nic1.ip_address, Some("192.168.1.101".to_string()));

        assert_eq!(attrs.mac_address, Some("52:54:00:12:34:56".to_string()));
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
            bmc: None,
        };
        let state = ServerState::new("test", &config);

        let attrs = build_attributes(&state.hardware, &state.server_name, &state);

        assert_eq!(attrs.network_interfaces.len(), 2);
        assert_eq!(attrs.network_interfaces[0].ip_address, None);
        assert_eq!(attrs.network_interfaces[1].ip_address, None);
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

        assert_eq!(attrs.disks.len(), 2);

        let nvme = &attrs.disks[0];
        assert_eq!(nvme.name, "nvme0n1");
        assert_eq!(nvme.size, Some(960));
        assert_eq!(nvme.disk_type, Some(DiskType::Nvme));
        assert_eq!(nvme.model, Some("Samsung 970 EVO".to_string()));
        assert!(nvme.serial.as_ref().unwrap().starts_with("SIM"));
        assert_eq!(
            nvme.path,
            Some("/dev/disk/by-path/pci-0000:01:00.0-nvme-1".to_string())
        );

        let ssd = &attrs.disks[1];
        assert_eq!(ssd.name, "sda");
        assert_eq!(ssd.size, Some(1920));
        assert_eq!(ssd.disk_type, Some(DiskType::Ssd));
        assert_eq!(ssd.model, Some("Samsung 860 EVO".to_string()));
        assert!(ssd.serial.as_ref().unwrap().starts_with("SIM"));
        assert_eq!(
            ssd.path,
            Some("/dev/disk/by-path/pci-0000:00:1f.2-ata-2".to_string())
        );
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

        assert_eq!(attrs.network_interfaces.len(), 2);
        assert_eq!(attrs.network_interfaces[0].speed_mbps, Some(10000));
        assert_eq!(attrs.network_interfaces[1].speed_mbps, Some(25000));
    }

    #[test]
    fn test_build_attributes_nic_count_mismatch() {
        use crate::hardware_profiles::NicConfig;

        let mut hardware = HardwareConfig::default();
        hardware.nics = vec![NicConfig { speed_mbps: 10000 }];

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

        assert_eq!(attrs.network_interfaces.len(), 2);
        assert_eq!(attrs.network_interfaces[0].speed_mbps, Some(10000));
        assert_eq!(attrs.network_interfaces[1].speed_mbps, None);
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

        assert_eq!(profile.disks.len(), 6);
        assert_eq!(profile.disks[0].name, "nvme0n1");
        assert_eq!(profile.disks[0].size_gb, 960);
        assert_eq!(profile.disks[1].name, "nvme1n1");
        assert_eq!(profile.disks[2].name, "sda");

        assert_eq!(profile.nics.len(), 2);
        assert_eq!(profile.nics[0].speed_mbps, 10000);
        assert_eq!(profile.nics[1].speed_mbps, 10000);
    }
}
