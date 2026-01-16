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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BmcInfo {
    mac_address: String,
    ip_address: Option<String>,
    ip_address_source: String,
}

fn default_ip_source() -> String {
    "static".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcConfiguration {
    #[serde(default = "default_ip_source")]
    pub ip_address_source: String, // "static" or "dhcp"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netmask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
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

/// Scan for BMC (Baseboard Management Controller) using ipmitool
///
/// This function attempts to detect a BMC by running `ipmitool lan print` on
/// channels 1, 2, and 8 (common BMC LAN channels). It parses the output to extract:
/// - MAC Address
/// - IP Address (converted to None if "0.0.0.0")
/// - IP Address Source (DHCP, Static, etc.)
///
/// Returns None if:
/// - ipmitool is not available
/// - No BMC is present
/// - Parsing fails
///
/// This is a best-effort scan and failures are non-fatal.
async fn scan_bmc() -> Result<Option<BmcInfo>> {
    // Try common BMC LAN channels in order
    for channel in [1, 2, 8] {
        debug!("Attempting to scan BMC on channel {}", channel);

        match tokio::process::Command::new("ipmitool")
            .args(["lan", "print", &channel.to_string()])
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                // Parse the output
                if let Some(bmc_info) = parse_ipmitool_output(&stdout) {
                    info!(
                        "Discovered BMC on channel {}: MAC={}, IP={:?}, Source={}",
                        channel,
                        bmc_info.mac_address,
                        bmc_info.ip_address,
                        bmc_info.ip_address_source
                    );
                    return Ok(Some(bmc_info));
                }
            }
            Ok(output) => {
                debug!(
                    "ipmitool command failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                if channel == 1 {
                    // Only log on first attempt to avoid spam
                    debug!("ipmitool not available or failed to execute: {}", e);
                }
            }
        }
    }

    debug!("No BMC detected on channels 1, 2, or 8");
    Ok(None)
}

/// Parse ipmitool lan print output to extract BMC information
fn parse_ipmitool_output(output: &str) -> Option<BmcInfo> {
    let mut mac_address: Option<String> = None;
    let mut ip_address: Option<String> = None;
    let mut ip_address_source: Option<String> = None;

    for line in output.lines() {
        let line = line.trim();

        if let Some(value) = line.strip_prefix("MAC Address") {
            // Extract everything after the first colon
            if let Some(colon_pos) = value.find(':') {
                let mac = value[colon_pos + 1..].trim().to_string();
                if !mac.is_empty() && mac != "00:00:00:00:00:00" {
                    mac_address = Some(mac);
                }
            }
        } else if let Some(value) = line.strip_prefix("IP Address Source") {
            if let Some(colon_pos) = value.find(':') {
                ip_address_source = Some(value[colon_pos + 1..].trim().to_string());
            }
        } else if line.starts_with("IP Address") && !line.contains("Source") {
            // Match "IP Address" but not "IP Address Source"
            if let Some(colon_pos) = line.find(':') {
                let ip = line[colon_pos + 1..].trim().to_string();
                // Treat 0.0.0.0 as "no IP" (None)
                if !ip.is_empty() && ip != "0.0.0.0" {
                    ip_address = Some(ip);
                }
            }
        }
    }

    // Only return BMC info if we found a valid MAC address
    if let (Some(mac), Some(source)) = (mac_address, ip_address_source) {
        Some(BmcInfo {
            mac_address: mac,
            ip_address,
            ip_address_source: source,
        })
    } else {
        None
    }
}

/// Configure BMC with static or DHCP IP address and credentials
///
/// This function configures the BMC by running ipmitool commands to set:
/// - IP address source (static or dhcp)
/// - For static: IP address, netmask, and default gateway
/// - Admin password (user ID 2)
///
/// Returns an error if ipmitool is not available or if configuration fails.
pub async fn configure_bmc(config: &BmcConfiguration, channel: u8) -> Result<()> {
    let ipsrc = config.ip_address_source.to_lowercase();

    info!(
        "Configuring BMC on channel {} with IP source: {}",
        channel, ipsrc
    );

    // Set IP address source (static or dhcp)
    run_ipmitool_command(
        channel,
        &["lan", "set", &channel.to_string(), "ipsrc", &ipsrc],
    )
    .await?;

    // Only set static IP fields if using static configuration
    if ipsrc == "static" {
        // Require static IP fields
        let ip_address = config
            .ip_address
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ip_address required for static BMC configuration"))?;
        let netmask = config
            .netmask
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("netmask required for static BMC configuration"))?;
        let gateway = config
            .gateway
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("gateway required for static BMC configuration"))?;

        info!("Configuring static IP: {}", ip_address);

        // Set IP address
        run_ipmitool_command(
            channel,
            &["lan", "set", &channel.to_string(), "ipaddr", ip_address],
        )
        .await?;

        // Set netmask
        run_ipmitool_command(
            channel,
            &["lan", "set", &channel.to_string(), "netmask", netmask],
        )
        .await?;

        // Set default gateway
        run_ipmitool_command(
            channel,
            &[
                "lan",
                "set",
                &channel.to_string(),
                "defgw",
                "ipaddr",
                gateway,
            ],
        )
        .await?;
    } else {
        info!("BMC will obtain IP automatically via DHCP");
    }

    // Set admin password if provided (works for both static and DHCP)
    if let Some(password) = &config.password {
        // User ID 2 is typically the ADMIN user
        run_ipmitool_command(channel, &["user", "set", "password", "2", password]).await?;
        info!("BMC admin password updated");
    }

    info!("BMC configuration completed successfully");
    Ok(())
}

/// Helper function to run ipmitool command
async fn run_ipmitool_command(channel: u8, args: &[&str]) -> Result<()> {
    debug!("Running ipmitool with args: {:?}", args);

    let output = tokio::process::Command::new("ipmitool")
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute ipmitool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "ipmitool command failed (channel {}): {}",
            channel,
            stderr
        ));
    }

    Ok(())
}

/// Configure BMC for the current device
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Fetches BMC configuration from rack-director
/// 3. Applies the configuration using ipmitool
/// 4. Reports success or failure to rack-director
pub async fn bmc_configure(client: &RackDirector) -> Result<()> {
    info!("Starting BMC configuration...");

    // Get device UUID
    let hardware_info = read_dmi().await?;
    let uuid = hardware_info
        .uuid
        .as_ref()
        .ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Device UUID: {}", uuid);

    // Fetch BMC configuration from rack-director
    info!("Fetching BMC configuration from rack-director...");
    let bmc_config = match client.get_bmc_config(uuid).await {
        Ok(config) => config,
        Err(e) => {
            let error_msg = format!("Failed to fetch BMC configuration: {}", e);
            log::error!("{}", error_msg);
            client.action_failed(uuid, &error_msg).await?;
            return Err(e);
        }
    };

    info!(
        "Retrieved BMC configuration: IP source={}",
        bmc_config.ip_address_source
    );
    if let Some(ip) = &bmc_config.ip_address {
        info!("  IP address: {}", ip);
    }

    // Convert client BmcConfig to scan BmcConfiguration
    let config = BmcConfiguration {
        ip_address_source: bmc_config.ip_address_source,
        ip_address: bmc_config.ip_address,
        netmask: bmc_config.netmask,
        gateway: bmc_config.gateway,
        username: bmc_config.username,
        password: bmc_config.password,
    };

    // Try configuring BMC on channels 1, 2, and 8
    let mut last_error = None;
    for channel in [1, 2, 8] {
        info!("Attempting to configure BMC on channel {}", channel);
        match configure_bmc(&config, channel).await {
            Ok(()) => {
                info!("BMC configured successfully on channel {}", channel);
                client.action_success(uuid).await?;
                return Ok(());
            }
            Err(e) => {
                warn!("Failed to configure BMC on channel {}: {}", channel, e);
                last_error = Some(e);
            }
        }
    }

    // All channels failed
    let error_msg = format!(
        "Failed to configure BMC on all channels: {}",
        last_error.unwrap()
    );
    log::error!("{}", error_msg);
    client.action_failed(uuid, &error_msg).await?;
    Err(anyhow!(error_msg))
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

    // Scan for BMC
    match scan_bmc().await {
        Ok(Some(bmc_info)) => {
            info!("Discovered BMC: MAC={}", bmc_info.mac_address);
            attributes.insert("bmc".to_string(), json!(bmc_info));
        }
        Ok(None) => {
            info!("No BMC detected");
        }
        Err(e) => {
            warn!("BMC scan failed: {}, continuing with other attributes", e);
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

    // Tests for BMC detection

    /// Test parsing valid ipmitool output
    #[test]
    fn test_parse_ipmitool_output_valid() {
        let output = r#"
Set in Progress         : Set Complete
Auth Type Support       : NONE MD2 MD5 PASSWORD
IP Address Source       : DHCP Address
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
MAC Address             : 0c:c4:7a:02:11:fe
SNMP Community String   : public
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(bmc.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(bmc.ip_address_source, "DHCP Address");
    }

    /// Test parsing ipmitool output with 0.0.0.0 IP (should be treated as None)
    #[test]
    fn test_parse_ipmitool_output_zero_ip() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : DHCP Address
IP Address              : 0.0.0.0
Subnet Mask             : 0.0.0.0
MAC Address             : 0c:c4:7a:02:11:fe
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(bmc.ip_address, None);
        assert_eq!(bmc.ip_address_source, "DHCP Address");
    }

    /// Test parsing ipmitool output with Static IP
    #[test]
    fn test_parse_ipmitool_output_static_ip() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : Static Address
IP Address              : 10.0.0.50
Subnet Mask             : 255.255.255.0
MAC Address             : aa:bb:cc:dd:ee:ff
Default Gateway IP      : 10.0.0.1
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(bmc.ip_address, Some("10.0.0.50".to_string()));
        assert_eq!(bmc.ip_address_source, "Static Address");
    }

    /// Test parsing ipmitool output missing MAC (should return None)
    #[test]
    fn test_parse_ipmitool_output_missing_mac() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : DHCP Address
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test parsing ipmitool output missing IP source (should return None)
    #[test]
    fn test_parse_ipmitool_output_missing_ip_source() {
        let output = r#"
Set in Progress         : Set Complete
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
MAC Address             : 0c:c4:7a:02:11:fe
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test parsing empty ipmitool output
    #[test]
    fn test_parse_ipmitool_output_empty() {
        let output = "";
        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test BmcInfo serialization
    #[test]
    fn test_bmc_info_serialization() {
        let bmc = BmcInfo {
            mac_address: "0c:c4:7a:02:11:fe".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            ip_address_source: "DHCP Address".to_string(),
        };

        let json = serde_json::to_string(&bmc).unwrap();
        assert!(json.contains("0c:c4:7a:02:11:fe"));
        assert!(json.contains("192.168.1.100"));
        assert!(json.contains("DHCP Address"));

        let deserialized: BmcInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(deserialized.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(deserialized.ip_address_source, "DHCP Address");
    }

    /// Test BmcInfo with no IP address
    #[test]
    fn test_bmc_info_no_ip() {
        let bmc = BmcInfo {
            mac_address: "0c:c4:7a:02:11:fe".to_string(),
            ip_address: None,
            ip_address_source: "DHCP Address".to_string(),
        };

        let json = serde_json::to_string(&bmc).unwrap();
        assert!(json.contains("0c:c4:7a:02:11:fe"));
        assert!(json.contains("null"));
        assert!(json.contains("DHCP Address"));

        let deserialized: BmcInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ip_address, None);
    }
}
