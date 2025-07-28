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

pub async fn device_scan(client: &RackDirector, scan_args: &DeviceScanArgs) -> Result<()> {
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
            dmidecode::Structure::Bios(bios) => todo!(),
            dmidecode::Structure::System(system) => todo!(),
            dmidecode::Structure::BaseBoard(base_board) => todo!(),
            dmidecode::Structure::Enclosure(enclosure) => todo!(),
            dmidecode::Structure::Processor(processor) => todo!(),
            dmidecode::Structure::Cache(cache) => todo!(),
            dmidecode::Structure::PortConnector(port_connector) => todo!(),
            dmidecode::Structure::SystemSlots(system_slots) => todo!(),
            dmidecode::Structure::OemStrings(oem_strings) => todo!(),
            dmidecode::Structure::SystemConfigurationOptions(system_configuration_options) => {
                todo!()
            }
            dmidecode::Structure::BiosLanguage(bios_language) => todo!(),
            dmidecode::Structure::GroupAssociations(group_associations) => todo!(),
            dmidecode::Structure::SystemEventLog(system_event_log) => todo!(),
            dmidecode::Structure::MemoryDevice(memory_device) => todo!(),
            dmidecode::Structure::MemoryError32(memory_error32) => todo!(),
            dmidecode::Structure::MemoryArrayMappedAddress(memory_array_mapped_address) => todo!(),
            dmidecode::Structure::MemoryDeviceMappedAddress(memory_device_mapped_address) => {
                todo!()
            }
            dmidecode::Structure::BuiltInPointingDevice(built_in_pointing_device) => todo!(),
            dmidecode::Structure::PortableBattery(portable_battery) => todo!(),
            dmidecode::Structure::PhysicalMemoryArray(physical_memory_array) => todo!(),
            dmidecode::Structure::Other(raw_structure) => todo!(),
        }
    }

    Ok(())
}
