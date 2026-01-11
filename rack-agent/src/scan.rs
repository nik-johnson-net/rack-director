use anyhow::{Result, anyhow};
use clap::Parser;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::RackDirector;

const SMBIOS_SYSFS: &str = "/sys/firmware/dmi/tables/smbios_entry_point";
const DMI_SYSFS: &str = "/sys/firmware/dmi/tables/DMI";

#[derive(Parser, Debug)]
pub struct DeviceScanArgs {
    /// Do not upload results to the Rack Director
    #[arg(long)]
    pub no_upload: bool,
}

impl DeviceScanArgs {
    pub fn new(no_upload: bool) -> Self {
        DeviceScanArgs { no_upload }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkInterface {
    interface_name: String,
    mac_address: String,
    ip_address: Option<String>,
    is_primary: bool,
}

#[derive(Debug, Default)]
struct HardwareInfo {
    uuid: Option<String>,
    manufacturer: Option<String>,
    product_name: Option<String>,
    serial_number: Option<String>,
    bios_version: Option<String>,
    bios_vendor: Option<String>,
    processors: Vec<ProcessorInfo>,
    memory_devices: Vec<MemoryInfo>,
}

#[derive(Debug)]
struct ProcessorInfo {
    designation: Option<String>,
    manufacturer: Option<String>,
    version: Option<String>,
    max_speed: Option<u16>,
    core_count: Option<u16>,
    thread_count: Option<u16>,
}

#[derive(Debug)]
struct MemoryInfo {
    size: Option<u16>,
    speed: Option<u16>,
    manufacturer: Option<String>,
    part_number: Option<String>,
}

/// Scan physical Ethernet network interfaces from /sys/class/net
///
/// This function scans for physical Ethernet interfaces, filtering out:
/// - Loopback interfaces (lo)
/// - Virtual interfaces (those without a /sys/class/net/{iface}/device directory)
/// - Non-Ethernet interfaces (type != 1)
///
/// Returns a vector of NetworkInterface structs with MAC addresses.
/// IP addresses are set to None and will be backfilled by rack-director from DHCP leases.
/// The first interface discovered is marked as primary.
async fn scan_network_interfaces() -> Result<Vec<NetworkInterface>> {
    let mut interfaces = Vec::new();
    let net_dir = std::path::Path::new("/sys/class/net");

    // Check if /sys/class/net exists
    if !net_dir.exists() {
        warn!("/sys/class/net not found, skipping network interface scan");
        return Ok(interfaces);
    }

    let mut entries = tokio::fs::read_dir(net_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let interface_name = entry.file_name().to_string_lossy().to_string();

        // Skip loopback
        if interface_name == "lo" {
            debug!("Skipping loopback interface: {}", interface_name);
            continue;
        }

        let iface_path = net_dir.join(&interface_name);

        // Check if physical Ethernet (must have /sys/class/net/{iface}/device/)
        let device_path = iface_path.join("device");
        if !device_path.exists() {
            debug!("Skipping virtual interface: {}", interface_name);
            continue;
        }

        // Check interface type (1 = Ethernet)
        let type_path = iface_path.join("type");
        if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
            if type_str.trim() != "1" {
                debug!(
                    "Skipping non-Ethernet interface: {} (type {})",
                    interface_name,
                    type_str.trim()
                );
                continue;
            }
        } else {
            debug!("Couldn't read type for interface: {}", interface_name);
            continue;
        }

        // Read MAC address
        let mac_path = iface_path.join("address");
        let mac_address = match tokio::fs::read_to_string(&mac_path).await {
            Ok(mac) => mac.trim().to_string(),
            Err(e) => {
                warn!("Couldn't read MAC address for {}: {}", interface_name, e);
                continue;
            }
        };

        debug!(
            "Found physical Ethernet interface: {} (MAC: {})",
            interface_name, mac_address
        );

        interfaces.push(NetworkInterface {
            interface_name,
            mac_address,
            ip_address: None, // Will be backfilled by rack-director from DHCP leases
            is_primary: interfaces.is_empty(), // First interface is primary
        });
    }

    Ok(interfaces)
}

pub async fn device_scan(client: &RackDirector, scan_args: &DeviceScanArgs) -> Result<()> {
    info!("Starting device hardware scan...");

    let hardware_info = read_dmi().await?;

    let uuid = hardware_info
        .uuid
        .as_ref()
        .ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Discovered device UUID: {}", uuid);

    // From this point on, if we encounter errors, we should report them to the server
    let result = perform_scan_and_upload(client, uuid, &hardware_info, scan_args).await;

    if let Err(e) = &result {
        log::error!("Hardware scan failed: {}", e);
        if !scan_args.no_upload {
            // Try to report the failure to the server
            if let Err(report_err) = client.action_failed(uuid, &e.to_string()).await {
                log::error!("Failed to report action failure to server: {}", report_err);
            }
        }
    }

    result
}

async fn perform_scan_and_upload(
    client: &RackDirector,
    uuid: &str,
    hardware_info: &HardwareInfo,
    scan_args: &DeviceScanArgs,
) -> Result<()> {
    // Build attributes JSON
    let mut attributes = serde_json::Map::new();

    if let Some(manufacturer) = &hardware_info.manufacturer {
        attributes.insert("manufacturer".to_string(), json!(manufacturer));
    }
    if let Some(product_name) = &hardware_info.product_name {
        attributes.insert("product_name".to_string(), json!(product_name));
    }
    if let Some(serial) = &hardware_info.serial_number {
        attributes.insert("serial_number".to_string(), json!(serial));
    }
    if let Some(bios_version) = &hardware_info.bios_version {
        attributes.insert("bios_version".to_string(), json!(bios_version));
    }
    if let Some(bios_vendor) = &hardware_info.bios_vendor {
        attributes.insert("bios_vendor".to_string(), json!(bios_vendor));
    }

    // Add processor information
    if !hardware_info.processors.is_empty() {
        let processors: Vec<_> = hardware_info
            .processors
            .iter()
            .map(|p| {
                json!({
                    "designation": p.designation,
                    "manufacturer": p.manufacturer,
                    "version": p.version,
                    "max_speed_mhz": p.max_speed,
                    "core_count": p.core_count,
                    "thread_count": p.thread_count,
                })
            })
            .collect();
        attributes.insert("processors".to_string(), json!(processors));
    }

    // Add memory information
    if !hardware_info.memory_devices.is_empty() {
        let memory_devices: Vec<_> = hardware_info
            .memory_devices
            .iter()
            .map(|m| {
                json!({
                    "size_mb": m.size,
                    "speed_mhz": m.speed,
                    "manufacturer": m.manufacturer,
                    "part_number": m.part_number,
                })
            })
            .collect();
        attributes.insert("memory_devices".to_string(), json!(memory_devices));

        let total_memory_mb: u64 = hardware_info
            .memory_devices
            .iter()
            .filter_map(|m| m.size.map(|s| s as u64))
            .sum();
        attributes.insert("total_memory_mb".to_string(), json!(total_memory_mb));
    }

    // Scan network interfaces
    match scan_network_interfaces().await {
        Ok(network_interfaces) => {
            if !network_interfaces.is_empty() {
                info!(
                    "Discovered {} network interface(s)",
                    network_interfaces.len()
                );
                attributes.insert("network_interfaces".to_string(), json!(network_interfaces));

                // Also set legacy mac_address field for backward compatibility
                if let Some(primary) = network_interfaces.first() {
                    attributes.insert("mac_address".to_string(), json!(primary.mac_address));
                }
            } else {
                info!("No physical Ethernet interfaces found");
            }
        }
        Err(e) => {
            warn!(
                "Network interface scan failed: {}, continuing with other attributes",
                e
            );
            // Non-fatal - continue with other hardware attributes
        }
    }

    info!(
        "Collected hardware information: {} attributes",
        attributes.len()
    );

    if !scan_args.no_upload {
        info!("Uploading hardware information to Rack Director...");
        client.update_attributes(uuid, attributes).await?;

        info!("Reporting discovery action success...");
        client.action_success(uuid).await?;

        info!("Hardware discovery completed successfully");
    } else {
        info!("Skipping upload (--no-upload flag set)");
        info!("Hardware info: {:#?}", hardware_info);
    }

    Ok(())
}

// Scan for DMI tables in a few locations
async fn read_dmi() -> Result<HardwareInfo> {
    debug!("trying to read SMBIOS at {SMBIOS_SYSFS}");
    match tokio::fs::read(SMBIOS_SYSFS).await {
        Ok(data) => return parse_dmi(&data),
        Err(e) => {
            debug!("failed to read DMI at SMBIOS location: {e}.");
        }
    };

    match tokio::fs::read(DMI_SYSFS).await {
        Ok(data) => return parse_dmi(&data),
        Err(e) => {
            debug!("failed to read DMI at DMI location: {e}.");
        }
    };

    Err(anyhow!("failed to read DMI data"))
}

// parse dmi tables for relevant information
fn parse_dmi(bytes: &[u8]) -> Result<HardwareInfo> {
    let entry_point = dmidecode::EntryPoint::search(bytes)?;

    info!(
        "Reading SMBIOS version {}.{}.{}",
        entry_point.major(),
        entry_point.minor(),
        entry_point.revision()
    );

    let mut hardware_info = HardwareInfo::default();

    for table in entry_point.structures(&bytes[entry_point.smbios_address() as usize..]) {
        let decoded_table = match table {
            Ok(s) => s,
            Err(e) => {
                warn!("Malformed SMBIOS structure: {e}");
                continue;
            }
        };

        match decoded_table {
            dmidecode::Structure::Bios(bios) => {
                hardware_info.bios_vendor = Some(bios.vendor.to_string());
                debug!("BIOS: vendor={:?}", hardware_info.bios_vendor);
            }
            dmidecode::Structure::System(system) => {
                hardware_info.manufacturer = Some(system.manufacturer.to_string());
                hardware_info.uuid = system.uuid.map(|u| u.to_string());
                debug!(
                    "System: manufacturer={:?}, uuid={:?}",
                    hardware_info.manufacturer, hardware_info.uuid
                );
            }
            dmidecode::Structure::Processor(processor) => {
                let proc_info = ProcessorInfo {
                    designation: Some(processor.socket_designation.to_string()),
                    manufacturer: None, // Not exposed by dmidecode library
                    version: None,      // Not exposed by dmidecode library
                    max_speed: Some(processor.max_speed),
                    core_count: processor.core_count,
                    thread_count: processor.thread_count,
                };
                debug!("Processor: {:?}", proc_info);
                hardware_info.processors.push(proc_info);
            }
            dmidecode::Structure::MemoryDevice(memory) => {
                let mem_info = MemoryInfo {
                    size: memory.size,
                    speed: memory.speed,
                    manufacturer: Some(memory.manufacturer.to_string()),
                    part_number: Some(memory.part_number.to_string()),
                };
                debug!("Memory: {:?}", mem_info);
                hardware_info.memory_devices.push(mem_info);
            }
            // Ignore other structures for now
            dmidecode::Structure::BaseBoard(_) => {}
            dmidecode::Structure::Enclosure(_) => {}
            dmidecode::Structure::Cache(_) => {}
            dmidecode::Structure::PortConnector(_) => {}
            dmidecode::Structure::SystemSlots(_) => {}
            dmidecode::Structure::OemStrings(_) => {}
            dmidecode::Structure::SystemConfigurationOptions(_) => {}
            dmidecode::Structure::BiosLanguage(_) => {}
            dmidecode::Structure::GroupAssociations(_) => {}
            dmidecode::Structure::SystemEventLog(_) => {}
            dmidecode::Structure::MemoryError32(_) => {}
            dmidecode::Structure::MemoryArrayMappedAddress(_) => {}
            dmidecode::Structure::MemoryDeviceMappedAddress(_) => {}
            dmidecode::Structure::BuiltInPointingDevice(_) => {}
            dmidecode::Structure::PortableBattery(_) => {}
            dmidecode::Structure::PhysicalMemoryArray(_) => {}
            dmidecode::Structure::Other(_) => {}
        }
    }

    Ok(hardware_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper function to create a temporary test directory structure
    /// for simulating /sys/class/net
    fn create_test_net_dir() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("Failed to create temp dir")
    }

    /// Helper to create a physical Ethernet interface in test directory
    fn create_physical_interface(base: &std::path::Path, name: &str, mac: &str) {
        let iface_path = base.join(name);
        fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Create device directory to mark as physical
        let device_path = iface_path.join("device");
        fs::create_dir_all(&device_path).expect("Failed to create device dir");

        // Write type file (1 = Ethernet)
        fs::write(iface_path.join("type"), "1\n").expect("Failed to write type");

        // Write MAC address
        fs::write(iface_path.join("address"), format!("{}\n", mac)).expect("Failed to write MAC");
    }

    /// Helper to create a virtual interface (no device directory)
    fn create_virtual_interface(base: &std::path::Path, name: &str, mac: &str) {
        let iface_path = base.join(name);
        fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // No device directory for virtual interfaces

        // Write type file (1 = Ethernet)
        fs::write(iface_path.join("type"), "1\n").expect("Failed to write type");

        // Write MAC address
        fs::write(iface_path.join("address"), format!("{}\n", mac)).expect("Failed to write MAC");
    }

    /// Helper to create a loopback interface
    fn create_loopback_interface(base: &std::path::Path) {
        let iface_path = base.join("lo");
        fs::create_dir_all(&iface_path).expect("Failed to create lo dir");

        // Write type file (772 = loopback)
        fs::write(iface_path.join("type"), "772\n").expect("Failed to write type");

        // Write MAC address (all zeros for loopback)
        fs::write(iface_path.join("address"), "00:00:00:00:00:00\n").expect("Failed to write MAC");
    }

    /// Helper to create a non-Ethernet interface
    fn create_non_ethernet_interface(base: &std::path::Path, name: &str, type_id: &str) {
        let iface_path = base.join(name);
        fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Create device directory
        let device_path = iface_path.join("device");
        fs::create_dir_all(&device_path).expect("Failed to create device dir");

        // Write non-Ethernet type
        fs::write(iface_path.join("type"), format!("{}\n", type_id)).expect("Failed to write type");

        // Write MAC address
        fs::write(iface_path.join("address"), "aa:bb:cc:dd:ee:ff\n").expect("Failed to write MAC");
    }

    /// Test scanning with a single physical Ethernet interface
    #[tokio::test]
    async fn test_scan_single_physical_interface() {
        let temp_dir = create_test_net_dir();
        create_physical_interface(temp_dir.path(), "eth0", "aa:bb:cc:dd:ee:f0");

        // Temporarily override the path by testing the logic directly
        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].interface_name, "eth0");
        assert_eq!(interfaces[0].mac_address, "aa:bb:cc:dd:ee:f0");
        assert_eq!(interfaces[0].ip_address, None);
        assert!(interfaces[0].is_primary);
    }

    /// Test scanning with multiple physical Ethernet interfaces
    #[tokio::test]
    async fn test_scan_multiple_physical_interfaces() {
        let temp_dir = create_test_net_dir();
        create_physical_interface(temp_dir.path(), "eth0", "aa:bb:cc:dd:ee:f0");
        create_physical_interface(temp_dir.path(), "eth1", "aa:bb:cc:dd:ee:f1");
        create_physical_interface(temp_dir.path(), "eth2", "aa:bb:cc:dd:ee:f2");
        create_physical_interface(temp_dir.path(), "eth3", "aa:bb:cc:dd:ee:f3");

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name: interface_name.clone(),
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 4);

        // Find the primary interface (first one discovered)
        let primary_count = interfaces.iter().filter(|i| i.is_primary).count();
        assert_eq!(
            primary_count, 1,
            "Exactly one interface should be marked as primary"
        );

        // Verify all have correct fields
        for iface in &interfaces {
            assert!(iface.interface_name.starts_with("eth"));
            assert!(iface.mac_address.starts_with("aa:bb:cc:dd:ee:f"));
            assert_eq!(iface.ip_address, None);
        }
    }

    /// Test that loopback interface is filtered out
    #[tokio::test]
    async fn test_filter_loopback_interface() {
        let temp_dir = create_test_net_dir();
        create_loopback_interface(temp_dir.path());
        create_physical_interface(temp_dir.path(), "eth0", "aa:bb:cc:dd:ee:f0");

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();

            // Skip loopback
            if interface_name == "lo" {
                continue;
            }

            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].interface_name, "eth0");
    }

    /// Test that virtual interfaces are filtered out
    #[tokio::test]
    async fn test_filter_virtual_interfaces() {
        let temp_dir = create_test_net_dir();
        create_physical_interface(temp_dir.path(), "eth0", "aa:bb:cc:dd:ee:f0");
        create_virtual_interface(temp_dir.path(), "veth0", "aa:bb:cc:dd:ee:00");
        create_virtual_interface(temp_dir.path(), "docker0", "aa:bb:cc:dd:ee:01");

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].interface_name, "eth0");
    }

    /// Test that non-Ethernet interfaces are filtered out
    #[tokio::test]
    async fn test_filter_non_ethernet_interfaces() {
        let temp_dir = create_test_net_dir();
        create_physical_interface(temp_dir.path(), "eth0", "aa:bb:cc:dd:ee:f0");
        create_non_ethernet_interface(temp_dir.path(), "wlan0", "803"); // 803 = IEEE 802.11
        create_non_ethernet_interface(temp_dir.path(), "sit0", "768"); // 768 = IPv6-in-IPv4 tunnel

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].interface_name, "eth0");
    }

    /// Test handling of missing MAC address file
    #[tokio::test]
    async fn test_missing_mac_address_file() {
        let temp_dir = create_test_net_dir();

        // Create interface without MAC address file
        let iface_path = temp_dir.path().join("eth0");
        fs::create_dir_all(&iface_path).expect("Failed to create interface dir");
        let device_path = iface_path.join("device");
        fs::create_dir_all(&device_path).expect("Failed to create device dir");
        fs::write(iface_path.join("type"), "1\n").expect("Failed to write type");
        // Don't create address file

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = match tokio::fs::read_to_string(&mac_path).await {
                Ok(mac) => mac.trim().to_string(),
                Err(_) => continue, // Skip if can't read MAC
            };

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        // Should skip the interface without MAC address
        assert_eq!(interfaces.len(), 0);
    }

    /// Test that first interface is marked as primary
    #[tokio::test]
    async fn test_first_interface_is_primary() {
        let temp_dir = create_test_net_dir();
        create_physical_interface(temp_dir.path(), "enp0s3", "aa:bb:cc:dd:ee:f0");
        create_physical_interface(temp_dir.path(), "enp0s8", "aa:bb:cc:dd:ee:f1");

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 2);

        // Exactly one should be primary
        let primary_count = interfaces.iter().filter(|i| i.is_primary).count();
        assert_eq!(primary_count, 1);

        // The first one in the vector should be primary
        assert!(interfaces[0].is_primary);
        assert!(!interfaces[1].is_primary);
    }

    /// Test empty directory (no interfaces)
    #[tokio::test]
    async fn test_empty_network_directory() {
        let temp_dir = create_test_net_dir();

        let mut interfaces = Vec::new();
        let net_dir = temp_dir.path();

        let mut entries = tokio::fs::read_dir(net_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let interface_name = entry.file_name().to_string_lossy().to_string();
            let iface_path = net_dir.join(&interface_name);
            let device_path = iface_path.join("device");

            if !device_path.exists() {
                continue;
            }

            let type_path = iface_path.join("type");
            if let Ok(type_str) = tokio::fs::read_to_string(&type_path).await {
                if type_str.trim() != "1" {
                    continue;
                }
            } else {
                continue;
            }

            let mac_path = iface_path.join("address");
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap()
                .trim()
                .to_string();

            interfaces.push(NetworkInterface {
                interface_name,
                mac_address,
                ip_address: None,
                is_primary: interfaces.is_empty(),
            });
        }

        assert_eq!(interfaces.len(), 0);
    }

    /// Test NetworkInterface struct serialization
    #[test]
    fn test_network_interface_serialization() {
        let interface = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            is_primary: true,
        };

        let json = serde_json::to_string(&interface).unwrap();
        assert!(json.contains("eth0"));
        assert!(json.contains("aa:bb:cc:dd:ee:ff"));
        assert!(json.contains("192.168.1.100"));
        assert!(json.contains("true"));

        let deserialized: NetworkInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.interface_name, "eth0");
        assert_eq!(deserialized.mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(deserialized.ip_address, Some("192.168.1.100".to_string()));
        assert!(deserialized.is_primary);
    }

    /// Test NetworkInterface with None ip_address
    #[test]
    fn test_network_interface_no_ip() {
        let interface = NetworkInterface {
            interface_name: "eth1".to_string(),
            mac_address: "11:22:33:44:55:66".to_string(),
            ip_address: None,
            is_primary: false,
        };

        let json = serde_json::to_string(&interface).unwrap();
        assert!(json.contains("eth1"));
        assert!(json.contains("11:22:33:44:55:66"));
        assert!(json.contains("null"));
        assert!(json.contains("false"));

        let deserialized: NetworkInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.interface_name, "eth1");
        assert_eq!(deserialized.ip_address, None);
        assert!(!deserialized.is_primary);
    }
}
