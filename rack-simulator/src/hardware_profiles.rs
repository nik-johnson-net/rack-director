use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

// TODO: Support BMCs in Hardware Profiles

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub manufacturer: Option<String>,
    pub product_name: Option<String>,
    pub serial_number: Option<String>,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub total_memory_mb: Option<u64>,
    pub processor_count: Option<u16>,
    pub cores_per_processor: Option<u16>,
    pub threads_per_core: Option<u16>,
    pub memory_dimm_count: Option<u16>,
    pub memory_dimm_size_mb: Option<u16>,
    pub memory_speed_mhz: Option<u16>,
}

pub fn get_profile(name: &str) -> Result<HardwareConfig> {
    match name.to_lowercase().as_str() {
        "dell-r640" => Ok(dell_r640()),
        "dell-r750" => Ok(dell_r750()),
        "hp-dl380" => Ok(hp_dl380()),
        "supermicro-x12" => Ok(supermicro_x12()),
        "generic" => Ok(generic()),
        _ => Err(anyhow!(
            "Unknown hardware profile: {}. Available: dell-r640, dell-r750, hp-dl380, supermicro-x12, generic",
            name
        )),
    }
}

pub fn generic() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("Generic".to_string()),
        product_name: Some("Server".to_string()),
        serial_number: Some("SN0000001".to_string()),
        bios_vendor: Some("Generic BIOS".to_string()),
        bios_version: Some("1.0.0".to_string()),
        total_memory_mb: Some(32768),
        processor_count: Some(1),
        cores_per_processor: Some(8),
        threads_per_core: Some(2),
        memory_dimm_count: Some(4),
        memory_dimm_size_mb: Some(8192),
        memory_speed_mhz: Some(2400),
    }
}

pub fn dell_r640() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("Dell Inc.".to_string()),
        product_name: Some("PowerEdge R640".to_string()),
        serial_number: Some("DELLSN00001".to_string()),
        bios_vendor: Some("Dell Inc.".to_string()),
        bios_version: Some("2.10.0".to_string()),
        total_memory_mb: Some(131072),
        processor_count: Some(2),
        cores_per_processor: Some(16),
        threads_per_core: Some(2),
        memory_dimm_count: Some(8),
        memory_dimm_size_mb: Some(16384),
        memory_speed_mhz: Some(2933),
    }
}

pub fn dell_r750() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("Dell Inc.".to_string()),
        product_name: Some("PowerEdge R750".to_string()),
        serial_number: Some("DELLSN00002".to_string()),
        bios_vendor: Some("Dell Inc.".to_string()),
        bios_version: Some("1.8.2".to_string()),
        total_memory_mb: Some(262144),
        processor_count: Some(2),
        cores_per_processor: Some(24),
        threads_per_core: Some(2),
        memory_dimm_count: Some(16),
        memory_dimm_size_mb: Some(16384),
        memory_speed_mhz: Some(3200),
    }
}

pub fn hp_dl380() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("HPE".to_string()),
        product_name: Some("ProLiant DL380 Gen10".to_string()),
        serial_number: Some("HPESN00001".to_string()),
        bios_vendor: Some("HPE".to_string()),
        bios_version: Some("U30 v2.68".to_string()),
        total_memory_mb: Some(131072),
        processor_count: Some(2),
        cores_per_processor: Some(18),
        threads_per_core: Some(2),
        memory_dimm_count: Some(8),
        memory_dimm_size_mb: Some(16384),
        memory_speed_mhz: Some(2666),
    }
}

pub fn supermicro_x12() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("Supermicro".to_string()),
        product_name: Some("X12SPL-F".to_string()),
        serial_number: Some("SMSN00001".to_string()),
        bios_vendor: Some("American Megatrends Inc.".to_string()),
        bios_version: Some("1.4".to_string()),
        total_memory_mb: Some(65536),
        processor_count: Some(1),
        cores_per_processor: Some(16),
        threads_per_core: Some(2),
        memory_dimm_count: Some(4),
        memory_dimm_size_mb: Some(16384),
        memory_speed_mhz: Some(3200),
    }
}
