use anyhow::{Result, anyhow};
use clap::Parser;
use log::{debug, info, warn};
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
    core_count: Option<u8>,
    thread_count: Option<u8>,
}

#[derive(Debug)]
struct MemoryInfo {
    size: Option<u64>,
    speed: Option<u16>,
    manufacturer: Option<String>,
    part_number: Option<String>,
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
            .filter_map(|m| m.size)
            .sum();
        attributes.insert("total_memory_mb".to_string(), json!(total_memory_mb));
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
                    manufacturer: None,
                    version: None,
                    max_speed: None,
                    core_count: None,
                    thread_count: None,
                };
                debug!("Processor: {:?}", proc_info);
                hardware_info.processors.push(proc_info);
            }
            dmidecode::Structure::MemoryDevice(_memory) => {
                let mem_info = MemoryInfo {
                    size: None,
                    speed: None,
                    manufacturer: None,
                    part_number: None,
                };
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
