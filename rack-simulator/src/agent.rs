use std::net::Ipv4Addr;

use anyhow::Result;
use serde_json::{Value, json};

use crate::ConnectionConfig;
use crate::hardware_profiles::HardwareConfig;
use crate::http::HttpClient;
use crate::output::Output;
use crate::server::ServerState;

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

    let processors: Vec<_> = (0..processor_count)
        .map(|i| {
            json!({
                "designation": format!("CPU{}", i),
                "manufacturer": null,
                "version": null,
                "max_speed_mhz": 2400,
                "core_count": cores,
                "thread_count": cores * threads,
            })
        })
        .collect();
    attrs.insert("processors".to_string(), json!(processors));

    let dimm_count = hardware.memory_dimm_count.unwrap_or(4);
    let dimm_size = hardware.memory_dimm_size_mb.unwrap_or(8192);
    let dimm_speed = hardware.memory_speed_mhz.unwrap_or(2400);

    let memory_devices: Vec<_> = (0..dimm_count)
        .map(|i| {
            json!({
                "size_mb": dimm_size,
                "speed_mhz": dimm_speed,
                "manufacturer": "Samsung",
                "part_number": format!("M393A2K40DB3-CWE-{}", i),
            })
        })
        .collect();
    attrs.insert("memory_devices".to_string(), json!(memory_devices));

    let total_memory = hardware
        .total_memory_mb
        .unwrap_or((dimm_count as u64) * (dimm_size as u64));
    attrs.insert("total_memory_mb".to_string(), json!(total_memory));

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

            json!({
                "interface_name": format!("eth{}", idx),
                "mac_address": mac_string,
                "ip_address": ip_address,
                "is_primary": idx == 0
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
        assert_eq!(nic0["is_primary"], true);

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
        assert_eq!(nic0["is_primary"], true);

        let nic1 = &network_interfaces[1];
        assert_eq!(nic1["interface_name"], "eth1");
        assert_eq!(nic1["mac_address"], "52:54:00:12:34:57");
        assert_eq!(nic1["ip_address"], "192.168.1.101");
        assert_eq!(nic1["is_primary"], false);

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
}
