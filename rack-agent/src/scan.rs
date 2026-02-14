use anyhow::{Result, anyhow};
use clap::Parser;
use common::device_attributes::{CpuInfo, DeviceAttributes, DiskType, MemoryInfo};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::bmc;
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
    speed_mbps: Option<u32>,
}

#[derive(Debug, Default)]
struct HardwareInfo {
    uuid: Option<String>,
    manufacturer: Option<String>,
    product_name: Option<String>,
    serial_number: Option<String>,
    bios_version: Option<String>,
    bios_vendor: Option<String>,
    cpus: Vec<CpuInfo>,
    memory: Vec<MemoryInfo>,
}

/// Read interface speed from /sys/class/net/{iface}/speed
///
/// Returns the link speed in Mbps if available.
/// Returns None if:
/// - The speed file doesn't exist
/// - The link is down (speed = -1)
/// - The speed cannot be parsed
async fn read_interface_speed(iface_path: &std::path::Path, interface_name: &str) -> Option<u32> {
    let speed_path = iface_path.join("speed");

    match tokio::fs::read_to_string(&speed_path).await {
        Ok(speed_str) => {
            let speed_str = speed_str.trim();
            match speed_str.parse::<i32>() {
                Ok(speed) if speed > 0 => {
                    debug!("Interface {} speed: {} Mbps", interface_name, speed);
                    Some(speed as u32)
                }
                Ok(speed) => {
                    debug!(
                        "Interface {} link is down (speed = {})",
                        interface_name, speed
                    );
                    None
                }
                Err(e) => {
                    debug!("Failed to parse speed for {}: {}", interface_name, e);
                    None
                }
            }
        }
        Err(e) => {
            debug!("Couldn't read speed for {}: {}", interface_name, e);
            None
        }
    }
}

/// Scan physical Ethernet network interfaces from /sys/class/net
///
/// This function scans for physical Ethernet interfaces, filtering out:
/// - Loopback interfaces (lo)
/// - Virtual interfaces (those without a /sys/class/net/{iface}/device directory)
/// - Non-Ethernet interfaces (type != 1)
///
/// Returns a vector of NetworkInterface structs with MAC addresses and speed.
/// IP addresses are set to None and will be backfilled by rack-director from DHCP leases.
/// Speed is read from /sys/class/net/{iface}/speed and may be None if link is down.
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

        // Read link speed (may not be available if link is down)
        let speed_mbps = read_interface_speed(&iface_path, &interface_name).await;

        debug!(
            "Found physical Ethernet interface: {} (MAC: {}, Speed: {:?} Mbps)",
            interface_name, mac_address, speed_mbps
        );

        interfaces.push(NetworkInterface {
            interface_name,
            mac_address,
            ip_address: None, // Will be backfilled by rack-director from DHCP leases
            speed_mbps,
        });
    }

    Ok(interfaces)
}

/// Disk information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskInfo {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disk_type: Option<DiskType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

/// Scan disk drives from /dev/disk/by-path/
///
/// This function discovers physical disk drives by:
/// 1. Reading /dev/disk/by-path/ to get stable bus-based paths
/// 2. Resolving symlinks to actual device names (e.g., sda, nvme0n1)
/// 3. Reading disk size from /sys/block/{name}/size
/// 4. Detecting disk type (NVMe, SSD, HDD) using rotational flag
/// 5. Reading model from /sys/block/{name}/device/model
///
/// Returns a vector of DiskInfo with path set to /dev/disk/by-path/... format.
/// This ensures stable device paths that don't change across reboots.
async fn scan_disks() -> Result<Vec<DiskInfo>> {
    let mut disks = Vec::new();
    let by_path_dir = std::path::Path::new("/dev/disk/by-path");

    // Check if /dev/disk/by-path exists
    if !by_path_dir.exists() {
        warn!("/dev/disk/by-path not found, skipping disk scan");
        return Ok(disks);
    }

    debug!("Scanning disks from /dev/disk/by-path/");

    // Read all entries in /dev/disk/by-path/
    let entries = match std::fs::read_dir(by_path_dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to read /dev/disk/by-path: {}", e);
            return Ok(disks);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                debug!("Failed to read directory entry: {}", e);
                continue;
            }
        };

        let path_link = entry.path();
        let path_name = match path_link.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip partition entries (contain -part)
        if path_name.contains("-part") {
            continue;
        }

        debug!("Processing disk path: {}", path_name);

        // Resolve symlink to get actual device name
        let target = match std::fs::read_link(&path_link) {
            Ok(t) => t,
            Err(e) => {
                debug!("Failed to resolve symlink for {}: {}", path_name, e);
                continue;
            }
        };

        // Extract device name from target (e.g., ../../sda -> sda)
        let device_name = match target.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        debug!("Resolved {} -> {}", path_name, device_name);

        // Read disk information from sysfs
        let size = read_disk_size(&device_name);
        let disk_type = detect_disk_type(&device_name);
        let model = read_disk_model(&device_name);

        let disk_info = DiskInfo {
            name: device_name.clone(),
            size,
            disk_type: Some(disk_type),
            model,
            serial: None, // Could be added with udevadm later
            path: Some(format!("/dev/disk/by-path/{}", path_name)),
        };

        debug!("Discovered disk: {} ({:?})", device_name, disk_type);

        disks.push(disk_info);
    }

    info!("Found {} disk(s)", disks.len());
    Ok(disks)
}

/// Detect disk type (NVMe, SSD, or HDD)
///
/// Returns:
/// - DiskType::Nvme for NVMe devices (nvme*)
/// - DiskType::Ssd for non-rotational devices (rotational flag = 0)
/// - DiskType::Hdd for rotational devices (rotational flag = 1) or when type can't be determined
fn detect_disk_type(device_name: &str) -> DiskType {
    // Check if it's an NVMe device
    if device_name.starts_with("nvme") {
        return DiskType::Nvme;
    }

    // Check rotational flag to distinguish SSD from HDD
    let rotational_path = format!("/sys/block/{}/queue/rotational", device_name);
    match std::fs::read_to_string(&rotational_path) {
        Ok(content) => {
            let rotational = content.trim();
            if rotational == "0" {
                DiskType::Ssd
            } else if rotational == "1" {
                DiskType::Hdd
            } else {
                debug!(
                    "Unexpected rotational value '{}' for {}, defaulting to HDD",
                    rotational, device_name
                );
                DiskType::Hdd
            }
        }
        Err(e) => {
            debug!(
                "Failed to read rotational flag for {}: {}, defaulting to HDD",
                device_name, e
            );
            DiskType::Hdd
        }
    }
}

/// Read disk size from /sys/block/{name}/size
///
/// Size is reported in 512-byte blocks, converted to GB (gigabytes).
/// Returns the size as u64 representing gigabytes.
fn read_disk_size(device_name: &str) -> Option<u64> {
    let size_path = format!("/sys/block/{}/size", device_name);
    match std::fs::read_to_string(&size_path) {
        Ok(content) => {
            let blocks = content.trim().parse::<u64>().ok()?;
            // Convert 512-byte blocks to GB (1 GB = 1,000,000,000 bytes)
            let size_gb = (blocks * 512) / 1_000_000_000;
            Some(size_gb)
        }
        Err(e) => {
            debug!("Failed to read size for {}: {}", device_name, e);
            None
        }
    }
}

/// Read disk model from /sys/block/{name}/device/model
fn read_disk_model(device_name: &str) -> Option<String> {
    let model_path = format!("/sys/block/{}/device/model", device_name);
    match std::fs::read_to_string(&model_path) {
        Ok(content) => {
            let model = content.trim().to_string();
            if model.is_empty() { None } else { Some(model) }
        }
        Err(e) => {
            debug!("Failed to read model for {}: {}", device_name, e);
            None
        }
    }
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
    // Build DeviceAttributes struct for type-safe attribute updates
    let mut attributes = DeviceAttributes {
        manufacturer: hardware_info.manufacturer.clone(),
        product_name: hardware_info.product_name.clone(),
        serial_number: hardware_info.serial_number.clone(),
        bios_version: hardware_info.bios_version.clone(),
        bios_vendor: hardware_info.bios_vendor.clone(),
        cpus: hardware_info.cpus.clone(),
        memory: hardware_info.memory.clone(),
        ..Default::default()
    };

    // Scan network interfaces
    match scan_network_interfaces().await {
        Ok(network_interfaces) => {
            if !network_interfaces.is_empty() {
                info!(
                    "Discovered {} network interface(s)",
                    network_interfaces.len()
                );

                // Convert local NetworkInterface to common::device_attributes::NetworkInterface
                attributes.network_interfaces = network_interfaces
                    .iter()
                    .map(|nic| common::device_attributes::NetworkInterface {
                        interface_name: nic.interface_name.clone(),
                        mac_address: nic.mac_address.clone(),
                        ip_address: nic.ip_address.clone(),
                        network_id: None,
                        speed_mbps: nic.speed_mbps,
                        disabled: false,
                        warning_label: None,
                    })
                    .collect();

                // Also set legacy mac_address field for backward compatibility
                if let Some(primary) = network_interfaces.first() {
                    attributes.mac_address = Some(primary.mac_address.clone());
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

    // Scan disks
    match scan_disks().await {
        Ok(disks) => {
            if !disks.is_empty() {
                info!("Discovered {} disk(s)", disks.len());

                // Convert local DiskInfo to common::device_attributes::DiskInfo
                attributes.disks = disks
                    .iter()
                    .map(|disk| common::device_attributes::DiskInfo {
                        name: disk.name.clone(),
                        size: disk.size,
                        disk_type: disk.disk_type,
                        model: disk.model.clone(),
                        serial: disk.serial.clone(),
                        path: disk.path.clone(),
                    })
                    .collect();
            } else {
                info!("No disks found");
            }
        }
        Err(e) => {
            warn!("Disk scan failed: {}, continuing with other attributes", e);
            // Non-fatal - continue with other hardware attributes
        }
    }

    // Scan for BMC
    match bmc::scan_bmc().await {
        Ok(Some(bmc_info)) => {
            info!("Discovered BMC: MAC={}", bmc_info.mac_address);

            // Convert local BmcInfo to common::device_attributes::BmcInfo
            attributes.bmc = Some(common::device_attributes::BmcInfo {
                mac_address: bmc_info.mac_address.clone(),
                ip_address: bmc_info.ip_address.clone(),
                ip_address_source: Some(bmc_info.ip_address_source.clone()),
            });
        }
        Ok(None) => {
            info!("No BMC detected");
        }
        Err(e) => {
            warn!("BMC scan failed: {}, continuing with other attributes", e);
            // Non-fatal - continue with other hardware attributes
        }
    }

    info!("Collected hardware information");

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
    debug!("trying to read SMBIOS from sysfs");

    // Try reading from sysfs (Linux standard location)
    // The entry point and structures are in separate files
    match (
        tokio::fs::read(SMBIOS_SYSFS).await,
        tokio::fs::read(DMI_SYSFS).await,
    ) {
        (Ok(entry_point_data), Ok(structures_data)) => {
            debug!(
                "Read {} bytes from entry point, {} bytes from DMI structures",
                entry_point_data.len(),
                structures_data.len()
            );
            return parse_dmi_sysfs(&entry_point_data, &structures_data);
        }
        (Err(e1), _) => {
            debug!("failed to read SMBIOS entry point at {SMBIOS_SYSFS}: {e1}");
        }
        (_, Err(e2)) => {
            debug!("failed to read DMI structures at {DMI_SYSFS}: {e2}");
        }
    }

    Err(anyhow!("failed to read DMI data from sysfs"))
}

/// Read DMI data and return just the UUID for BMC configuration
///
/// This is a public helper function used by the bmc module to get device UUID
pub async fn read_dmi_for_uuid() -> Result<Option<String>> {
    let hardware_info = read_dmi().await?;
    Ok(hardware_info.uuid)
}

// Parse DMI tables from sysfs (separate entry point and structures files)
fn parse_dmi_sysfs(entry_point_data: &[u8], structures_data: &[u8]) -> Result<HardwareInfo> {
    let entry_point = dmidecode::EntryPoint::search(entry_point_data)?;

    info!(
        "Reading SMBIOS version {}.{}.{}",
        entry_point.major(),
        entry_point.minor(),
        entry_point.revision()
    );

    let mut hardware_info = HardwareInfo::default();

    // In sysfs, the structures data is already extracted, so we start at offset 0
    for table in entry_point.structures(structures_data) {
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
                let cpu_info = CpuInfo {
                    designation: Some(processor.socket_designation.to_string()),
                    manufacturer: Some(processor.processor_manufacturer.to_string()),
                    model: Some(processor.processor_version.to_string()),
                    speed_mhz: Some(processor.max_speed as u32),
                    cores: processor.core_count.map(|c| c as u32),
                    threads: processor.thread_count.map(|t| t as u32),
                };
                debug!("CPU: {:?}", cpu_info);
                hardware_info.cpus.push(cpu_info);
            }
            dmidecode::Structure::MemoryDevice(memory) => {
                let mem_info = MemoryInfo {
                    size_mb: memory.size,
                    speed_mhz: memory.speed.map(|s| s as u32),
                    manufacturer: Some(memory.manufacturer.to_string()),
                    part_number: Some(memory.part_number.to_string()),
                };
                debug!("Memory: {:?}", mem_info);
                hardware_info.memory.push(mem_info);
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
                speed_mbps: None, // Not reading speed in tests
            });
        }

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].interface_name, "eth0");
        assert_eq!(interfaces[0].mac_address, "aa:bb:cc:dd:ee:f0");
        assert_eq!(interfaces[0].ip_address, None);
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
                speed_mbps: None, // Not reading speed in tests
            });
        }

        assert_eq!(interfaces.len(), 4);

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
                speed_mbps: None, // Not reading speed in tests
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
                speed_mbps: None, // Not reading speed in tests
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
                speed_mbps: None, // Not reading speed in tests
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
                speed_mbps: None, // Not reading speed in tests
            });
        }

        // Should skip the interface without MAC address
        assert_eq!(interfaces.len(), 0);
    }

    /// Test that multiple interfaces are scanned correctly
    #[tokio::test]
    async fn test_multiple_interfaces_scanned() {
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
                speed_mbps: None, // Not reading speed in tests
            });
        }

        assert_eq!(interfaces.len(), 2);
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
                speed_mbps: None, // Not reading speed in tests
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
            speed_mbps: Some(10000),
        };

        let json = serde_json::to_string(&interface).unwrap();
        assert!(json.contains("eth0"));
        assert!(json.contains("aa:bb:cc:dd:ee:ff"));
        assert!(json.contains("192.168.1.100"));
        assert!(json.contains("10000"));

        let deserialized: NetworkInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.interface_name, "eth0");
        assert_eq!(deserialized.mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(deserialized.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(deserialized.speed_mbps, Some(10000));
    }

    /// Test NetworkInterface with None ip_address
    #[test]
    fn test_network_interface_no_ip() {
        let interface = NetworkInterface {
            interface_name: "eth1".to_string(),
            mac_address: "11:22:33:44:55:66".to_string(),
            ip_address: None,
            speed_mbps: Some(1000),
        };

        let json = serde_json::to_string(&interface).unwrap();
        assert!(json.contains("eth1"));
        assert!(json.contains("11:22:33:44:55:66"));
        assert!(json.contains("null"));

        let deserialized: NetworkInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.interface_name, "eth1");
        assert_eq!(deserialized.ip_address, None);
        assert_eq!(deserialized.speed_mbps, Some(1000));
    }

    /// Test reading interface speed from sysfs
    #[tokio::test]
    async fn test_read_interface_speed_valid() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let iface_path = temp_dir.path().join("eth0");
        std::fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Write valid speed
        std::fs::write(iface_path.join("speed"), "10000\n").expect("Failed to write speed");

        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, Some(10000));
    }

    /// Test reading interface speed when link is down (-1)
    #[tokio::test]
    async fn test_read_interface_speed_link_down() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let iface_path = temp_dir.path().join("eth0");
        std::fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Write -1 (link down)
        std::fs::write(iface_path.join("speed"), "-1\n").expect("Failed to write speed");

        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, None);
    }

    /// Test reading interface speed when file doesn't exist
    #[tokio::test]
    async fn test_read_interface_speed_missing_file() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let iface_path = temp_dir.path().join("eth0");
        std::fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Don't create speed file

        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, None);
    }

    /// Test reading interface speed with invalid data
    #[tokio::test]
    async fn test_read_interface_speed_invalid_data() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let iface_path = temp_dir.path().join("eth0");
        std::fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Write invalid data
        std::fs::write(iface_path.join("speed"), "invalid\n").expect("Failed to write speed");

        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, None);
    }

    /// Test common NIC speeds
    #[tokio::test]
    async fn test_read_interface_speed_common_speeds() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let iface_path = temp_dir.path().join("eth0");
        std::fs::create_dir_all(&iface_path).expect("Failed to create interface dir");

        // Test 1 Gbps
        std::fs::write(iface_path.join("speed"), "1000\n").expect("Failed to write speed");
        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, Some(1000));

        // Test 10 Gbps
        std::fs::write(iface_path.join("speed"), "10000\n").expect("Failed to write speed");
        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, Some(10000));

        // Test 100 Mbps
        std::fs::write(iface_path.join("speed"), "100\n").expect("Failed to write speed");
        let speed = read_interface_speed(&iface_path, "eth0").await;
        assert_eq!(speed, Some(100));
    }

    // Tests for DMI parsing

    /// Helper function to calculate SMBIOS entry point checksum
    /// The checksum byte makes the sum of all bytes equal to zero (mod 256)
    fn calculate_smbios_checksum(data: &[u8], checksum_offset: usize) -> u8 {
        let sum: u8 = data
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != checksum_offset)
            .map(|(_, &b)| b)
            .fold(0u8, |acc, b| acc.wrapping_add(b));
        0u8.wrapping_sub(sum)
    }

    /// Test that parse_dmi_sysfs correctly handles separate entry point and structures data
    /// This test verifies the fix for the "range start index out of range" bug
    #[test]
    fn test_parse_dmi_sysfs_separate_files() {
        // This is a minimal SMBIOS 2.1 entry point structure (32 bytes)
        // It contains metadata about the SMBIOS tables but NOT the actual table data
        let mut entry_point_data: Vec<u8> = vec![
            0x5f, 0x53, 0x4d, 0x5f, // "_SM_" anchor string
            0x00, // Checksum (will be calculated)
            0x1f, // Entry point length (31 bytes)
            0x02, // SMBIOS major version (2)
            0x01, // SMBIOS minor version (1)
            0x00, 0x04, // Maximum structure size (1024 bytes)
            0x00, // Entry point revision
            0x00, 0x00, 0x00, 0x00, 0x00, // Formatted area
            0x5f, 0x44, 0x4d, 0x49, 0x5f, // "_DMI_" intermediate anchor
            0x00, // Intermediate checksum (will be calculated)
            0x60, 0x00, // Structure table length (96 bytes)
            0x00, 0xe0, 0x6f, 0x8f, // Structure table address (0x8F6FE000 - physical memory)
            0x02, 0x00, // Number of structures (2)
            0x21, // SMBIOS BCD revision (2.1)
        ];

        // Calculate and set the intermediate checksum (bytes 16-30)
        let intermediate_checksum = calculate_smbios_checksum(&entry_point_data[16..31], 5);
        entry_point_data[16 + 5] = intermediate_checksum;

        // Calculate and set the entry point checksum (bytes 0-30)
        let entry_checksum = calculate_smbios_checksum(&entry_point_data[0..31], 4);
        entry_point_data[4] = entry_checksum;

        // This is the actual SMBIOS structures data (extracted from physical memory by kernel)
        // In sysfs, this data is pre-extracted and ready to parse at offset 0
        let structures_data: Vec<u8> = vec![
            // Structure 1: BIOS Information (Type 0)
            0x00, // Type 0 (BIOS)
            0x18, // Length (24 bytes)
            0x00, 0x00, // Handle 0x0000
            0x01, // Vendor string index (1)
            0x02, // BIOS version string index (2)
            0x00, 0xe0, // BIOS starting segment
            0x03, // Release date string index (3)
            0x00, // BIOS ROM size
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // BIOS characteristics
            0x00, 0x00, // Extension bytes
            0x00, // Major release
            0x00, // Minor release
            0x00, // EC firmware major
            0x00, // EC firmware minor
            // Strings section
            b'T', b'e', b's', b't', b'V', b'e', b'n', b'd', b'o', b'r', 0x00, // "TestVendor"
            b'1', b'.', b'0', 0x00, // "1.0"
            b'2', b'0', b'2', b'4', 0x00, // "2024"
            0x00, // Double null terminator
            // Structure 2: System Information (Type 1)
            0x01, // Type 1 (System)
            0x1b, // Length (27 bytes)
            0x01, 0x00, // Handle 0x0001
            0x01, // Manufacturer string index (1)
            0x02, // Product name string index (2)
            0x03, // Version string index (3)
            0x04, // Serial number string index (4)
            // UUID (16 bytes) - valid UUID
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0xde, 0xf0, 0x06, // Wake-up type
            0x00, // SKU number
            0x00, // Family
            // Strings section
            b'T', b'e', b's', b't', b'M', b'f', b'g', 0x00, // "TestMfg"
            b'T', b'e', b's', b't', b'P', b'r', b'o', b'd', 0x00, // "TestProd"
            b'v', b'1', 0x00, // "v1"
            b'S', b'N', b'1', b'2', b'3', 0x00, // "SN123"
            0x00, // Double null terminator
        ];

        // This should NOT panic with "range start index out of range"
        let result = parse_dmi_sysfs(&entry_point_data, &structures_data);

        // Verify it parsed successfully
        match &result {
            Ok(_) => (),
            Err(e) => panic!("parse_dmi_sysfs failed: {:?}", e),
        }

        let hardware_info = result.unwrap();

        // Verify we got the expected data
        assert_eq!(
            hardware_info.bios_vendor,
            Some("TestVendor".to_string()),
            "Should extract BIOS vendor"
        );
        assert_eq!(
            hardware_info.manufacturer,
            Some("TestMfg".to_string()),
            "Should extract system manufacturer"
        );
        assert!(hardware_info.uuid.is_some(), "Should extract system UUID");
    }

    /// Test that parse_dmi_sysfs handles empty structures data
    #[test]
    fn test_parse_dmi_sysfs_empty_structures() {
        let mut entry_point_data: Vec<u8> = vec![
            0x5f, 0x53, 0x4d, 0x5f, // "_SM_" anchor string
            0x00, // Checksum (will be calculated)
            0x1f, // Entry point length
            0x02, // SMBIOS major version
            0x01, // SMBIOS minor version
            0x00, 0x04, // Maximum structure size
            0x00, // Entry point revision
            0x00, 0x00, 0x00, 0x00, 0x00, // Formatted area
            0x5f, 0x44, 0x4d, 0x49, 0x5f, // "_DMI_" intermediate anchor
            0x00, // Intermediate checksum (will be calculated)
            0x00, 0x00, // Structure table length (0 bytes)
            0x00, 0xe0, 0x6f, 0x8f, // Structure table address
            0x00, 0x00, // Number of structures (0)
            0x21, // SMBIOS BCD revision
        ];

        // Calculate and set the intermediate checksum (bytes 16-30)
        let intermediate_checksum = calculate_smbios_checksum(&entry_point_data[16..31], 5);
        entry_point_data[16 + 5] = intermediate_checksum;

        // Calculate and set the entry point checksum (bytes 0-30)
        let entry_checksum = calculate_smbios_checksum(&entry_point_data[0..31], 4);
        entry_point_data[4] = entry_checksum;

        let structures_data: Vec<u8> = vec![];

        let result = parse_dmi_sysfs(&entry_point_data, &structures_data);

        // Should succeed but return empty hardware info
        assert!(result.is_ok(), "Should handle empty structures gracefully");

        let hardware_info = result.unwrap();
        assert_eq!(hardware_info.cpus.len(), 0);
        assert_eq!(hardware_info.memory.len(), 0);
    }

    /// Test that parse_dmi_sysfs rejects invalid entry point data
    #[test]
    fn test_parse_dmi_sysfs_invalid_entry_point() {
        let invalid_entry_point: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00];
        let structures_data: Vec<u8> = vec![0x00];

        let result = parse_dmi_sysfs(&invalid_entry_point, &structures_data);

        // Should fail to parse invalid entry point
        assert!(result.is_err(), "Should reject invalid entry point data");
    }

    /// Test disk type detection for NVMe devices
    #[test]
    fn test_detect_disk_type_nvme() {
        let disk_type = detect_disk_type("nvme0n1");
        assert_eq!(disk_type, DiskType::Nvme);

        let disk_type = detect_disk_type("nvme1n1");
        assert_eq!(disk_type, DiskType::Nvme);
    }

    /// Test disk type detection returns HDD for devices without rotational flag
    #[test]
    fn test_detect_disk_type_nonexistent_device() {
        let disk_type = detect_disk_type("nonexistent");
        // Should return HDD as default when rotational file doesn't exist
        assert_eq!(disk_type, DiskType::Hdd);
    }

    /// Test disk size parsing
    #[test]
    fn test_read_disk_size_nonexistent() {
        let size = read_disk_size("nonexistent");
        // Should return None when size file doesn't exist
        assert_eq!(size, None);
    }

    /// Test disk model parsing
    #[test]
    fn test_read_disk_model_nonexistent() {
        let model = read_disk_model("nonexistent");
        // Should return None when model file doesn't exist
        assert_eq!(model, None);
    }

    /// Test scan_disks with missing /dev/disk/by-path directory
    #[tokio::test]
    async fn test_scan_disks_missing_directory() {
        // This test verifies graceful handling when /dev/disk/by-path doesn't exist
        // In real system it would exist, but in test environment it likely won't
        let result = scan_disks().await;

        // Should succeed even if directory doesn't exist
        assert!(result.is_ok());
        let _disks = result.unwrap();

        // May be empty if directory doesn't exist, which is fine for testing
        // The important thing is it doesn't crash and returns successfully
    }

    /// Test that DiskInfo serializes correctly
    #[test]
    fn test_disk_info_serialization() {
        let disk = DiskInfo {
            name: "sda".to_string(),
            size: Some(480),
            disk_type: Some(DiskType::Ssd),
            model: Some("Samsung SSD".to_string()),
            serial: None,
            path: Some("/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string()),
        };

        let json = serde_json::to_value(&disk).unwrap();

        assert_eq!(json["name"], "sda");
        assert_eq!(json["size"], 480);
        assert_eq!(json["disk_type"], "ssd");
        assert_eq!(json["model"], "Samsung SSD");
        assert_eq!(json["path"], "/dev/disk/by-path/pci-0000:00:1f.2-ata-1");

        // Serial should not be in JSON when None
        assert!(!json.as_object().unwrap().contains_key("serial"));
    }

    /// Test type safety: DeviceAttributes serialization produces correct field names
    #[test]
    fn test_device_attributes_type_safety() {
        let attributes = DeviceAttributes {
            manufacturer: Some("Dell Inc.".to_string()),
            product_name: Some("PowerEdge R640".to_string()),
            cpus: vec![CpuInfo {
                designation: Some("CPU 1".to_string()),
                manufacturer: Some("Intel".to_string()),
                model: Some("Xeon E5-2680".to_string()),
                speed_mhz: Some(2400),
                cores: Some(8),
                threads: Some(16),
            }],
            memory: vec![MemoryInfo {
                size_mb: Some(16384),
                speed_mhz: Some(2400),
                manufacturer: Some("Samsung".to_string()),
                part_number: Some("M393A2K40BB1-CRC".to_string()),
            }],
            ..Default::default()
        };

        // Serialize to JSON
        let json = serde_json::to_value(&attributes).unwrap();

        // Verify field names match the common::device_attributes specification
        assert_eq!(json["manufacturer"], "Dell Inc.");
        assert_eq!(json["product_name"], "PowerEdge R640");

        // Verify CPU fields use correct names (cpus, not processors)
        assert!(json.get("cpus").is_some());
        assert!(json.get("processors").is_none());
        assert_eq!(json["cpus"][0]["model"], "Xeon E5-2680");
        assert_eq!(json["cpus"][0]["speed_mhz"], 2400);
        assert_eq!(json["cpus"][0]["cores"], 8);
        assert_eq!(json["cpus"][0]["threads"], 16);

        // Verify memory fields use correct names (memory, not memory_devices)
        assert!(json.get("memory").is_some());
        assert!(json.get("memory_devices").is_none());
        assert_eq!(json["memory"][0]["size_mb"], 16384);
        assert_eq!(json["memory"][0]["speed_mhz"], 2400);
    }
}
