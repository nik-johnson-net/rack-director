use anyhow::{Result, anyhow};
use clap::Parser;
use log::{debug, info, warn};

use crate::client::RackDirector;

const SMBIOS_SYSFS: &str = "/sys/firmware/dmi/tables/smbios_entry_point";
const DMI_SYSFS: &str = "/sys/firmware/dmi/tables/DMI";

#[derive(Parser, Debug)]
pub struct DeviceScanArgs {
    // Do not upload results to the Rack Director
    #[arg(long)]
    no_upload: bool,
}

pub async fn device_scan(_client: &RackDirector, _scan_args: &DeviceScanArgs) -> Result<()> {
    read_dmi().await?;

    Ok(())
}

// Scan for DMI tables in a few locations
async fn read_dmi() -> Result<()> {
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
fn parse_dmi(bytes: &[u8]) -> Result<()> {
    let entry_point = dmidecode::EntryPoint::search(bytes)?;

    info!(
        "Reading SMBIOS version {}.{}.{}",
        entry_point.major(),
        entry_point.minor(),
        entry_point.revision()
    );

    for table in entry_point.structures(&bytes[entry_point.smbios_address() as usize..]) {
        let decoded_table = match table {
            Ok(s) => s,
            Err(e) => {
                warn!("Malformed SMBIOS structure: {e}");
                continue;
            }
        };

        match decoded_table {
            dmidecode::Structure::Bios(_bios) => todo!(),
            dmidecode::Structure::System(_system) => todo!(),
            dmidecode::Structure::BaseBoard(_base_board) => todo!(),
            dmidecode::Structure::Enclosure(_enclosure) => todo!(),
            dmidecode::Structure::Processor(_processor) => todo!(),
            dmidecode::Structure::Cache(_cache) => todo!(),
            dmidecode::Structure::PortConnector(_port_connector) => todo!(),
            dmidecode::Structure::SystemSlots(_system_slots) => todo!(),
            dmidecode::Structure::OemStrings(_oem_strings) => todo!(),
            dmidecode::Structure::SystemConfigurationOptions(_system_configuration_options) => {
                todo!()
            }
            dmidecode::Structure::BiosLanguage(_bios_language) => todo!(),
            dmidecode::Structure::GroupAssociations(_group_associations) => todo!(),
            dmidecode::Structure::SystemEventLog(_system_event_log) => todo!(),
            dmidecode::Structure::MemoryDevice(_memory_device) => todo!(),
            dmidecode::Structure::MemoryError32(_memory_error32) => todo!(),
            dmidecode::Structure::MemoryArrayMappedAddress(_memory_array_mapped_address) => todo!(),
            dmidecode::Structure::MemoryDeviceMappedAddress(_memory_device_mapped_address) => {
                todo!()
            }
            dmidecode::Structure::BuiltInPointingDevice(_built_in_pointing_device) => todo!(),
            dmidecode::Structure::PortableBattery(_portable_battery) => todo!(),
            dmidecode::Structure::PhysicalMemoryArray(_physical_memory_array) => todo!(),
            dmidecode::Structure::Other(_raw_structure) => todo!(),
        }
    }

    Ok(())
}
