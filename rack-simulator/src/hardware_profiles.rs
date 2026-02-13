use anyhow::{Result, anyhow};
use common::device_attributes::DiskType;
use serde::{Deserialize, Serialize};

// TODO: Support BMCs in Hardware Profiles

/// Disk configuration for a hardware profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    /// Disk device name (e.g., "sda", "nvme0n1")
    pub name: String,
    /// Disk size in gigabytes (GB)
    pub size_gb: u64,
    /// Disk type (nvme, ssd, or hdd)
    pub disk_type: DiskType,
    /// Disk model name
    pub model: String,
}

/// NIC configuration for a hardware profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NicConfig {
    /// NIC speed in megabits per second (e.g., 1000, 10000, 25000)
    pub speed_mbps: u32,
}

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
    /// CPU manufacturer (e.g., "Intel Corporation", "AMD")
    pub cpu_manufacturer: Option<String>,
    /// CPU model/version string (e.g., "Intel(R) Xeon(R) Gold 6248R")
    pub cpu_model: Option<String>,
    /// Disk configurations (devices will be assigned sequentially)
    #[serde(default)]
    pub disks: Vec<DiskConfig>,
    /// NIC configurations (must match the number of MAC addresses in the server config)
    #[serde(default)]
    pub nics: Vec<NicConfig>,
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
        cpu_manufacturer: Some("Intel Corporation".to_string()),
        cpu_model: Some("Intel(R) Core(TM) i7-8700".to_string()),
        disks: vec![
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 480,
                disk_type: DiskType::Ssd,
                model: "Generic SSD 480GB".to_string(),
            },
            DiskConfig {
                name: "sdb".to_string(),
                size_gb: 1000,
                disk_type: DiskType::Hdd,
                model: "Generic HDD 1TB".to_string(),
            },
        ],
        nics: vec![
            NicConfig { speed_mbps: 1000 },
            NicConfig { speed_mbps: 1000 },
        ],
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
        cpu_manufacturer: Some("Intel Corporation".to_string()),
        cpu_model: Some("Intel(R) Xeon(R) Gold 6248R".to_string()),
        disks: vec![
            DiskConfig {
                name: "nvme0n1".to_string(),
                size_gb: 960,
                disk_type: DiskType::Nvme,
                model: "Dell Express Flash PM1725b 960GB".to_string(),
            },
            DiskConfig {
                name: "nvme1n1".to_string(),
                size_gb: 960,
                disk_type: DiskType::Nvme,
                model: "Dell Express Flash PM1725b 960GB".to_string(),
            },
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 1.92TB".to_string(),
            },
            DiskConfig {
                name: "sdb".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 1.92TB".to_string(),
            },
            DiskConfig {
                name: "sdc".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 1.92TB".to_string(),
            },
            DiskConfig {
                name: "sdd".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 1.92TB".to_string(),
            },
        ],
        nics: vec![
            NicConfig { speed_mbps: 10000 },
            NicConfig { speed_mbps: 10000 },
        ],
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
        cpu_manufacturer: Some("Intel Corporation".to_string()),
        cpu_model: Some("Intel(R) Xeon(R) Platinum 8352Y".to_string()),
        disks: vec![
            DiskConfig {
                name: "nvme0n1".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Nvme,
                model: "Dell Express Flash PM1733 1.92TB".to_string(),
            },
            DiskConfig {
                name: "nvme1n1".to_string(),
                size_gb: 1920,
                disk_type: DiskType::Nvme,
                model: "Dell Express Flash PM1733 1.92TB".to_string(),
            },
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
            DiskConfig {
                name: "sdb".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
            DiskConfig {
                name: "sdc".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
            DiskConfig {
                name: "sdd".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
            DiskConfig {
                name: "sde".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
            DiskConfig {
                name: "sdf".to_string(),
                size_gb: 3840,
                disk_type: DiskType::Ssd,
                model: "Dell SSD 3.84TB".to_string(),
            },
        ],
        nics: vec![
            NicConfig { speed_mbps: 25000 },
            NicConfig { speed_mbps: 25000 },
        ],
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
        cpu_manufacturer: Some("Intel Corporation".to_string()),
        cpu_model: Some("Intel(R) Xeon(R) Gold 6240".to_string()),
        disks: vec![
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 480,
                disk_type: DiskType::Ssd,
                model: "HPE SSD 480GB".to_string(),
            },
            DiskConfig {
                name: "sdb".to_string(),
                size_gb: 480,
                disk_type: DiskType::Ssd,
                model: "HPE SSD 480GB".to_string(),
            },
            DiskConfig {
                name: "sdc".to_string(),
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                model: "HPE HDD 2TB".to_string(),
            },
            DiskConfig {
                name: "sdd".to_string(),
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                model: "HPE HDD 2TB".to_string(),
            },
            DiskConfig {
                name: "sde".to_string(),
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                model: "HPE HDD 2TB".to_string(),
            },
            DiskConfig {
                name: "sdf".to_string(),
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                model: "HPE HDD 2TB".to_string(),
            },
        ],
        nics: vec![
            NicConfig { speed_mbps: 10000 },
            NicConfig { speed_mbps: 10000 },
        ],
    }
}

pub fn supermicro_x12() -> HardwareConfig {
    HardwareConfig {
        manufacturer: Some("Supermicro".to_string()),
        product_name: Some("X12SPL-F".to_string()),
        serial_number: Some("SMSN00001".to_string()),
        bios_vendor: Some("American Megatrends Inc.".to_string()),
        bios_version: Some("1.4".to_string()),
        total_memory_mb: Some(262144),    // 256GB (was 65536)
        processor_count: Some(2),         // 2 processors (was 1)
        cores_per_processor: Some(32),    // 32 cores per processor (was 16)
        threads_per_core: Some(2),        // unchanged
        memory_dimm_count: Some(8),       // unchanged
        memory_dimm_size_mb: Some(32768), // 32GB each (was 16384)
        memory_speed_mhz: Some(3200),
        cpu_manufacturer: Some("Intel Corporation".to_string()),
        cpu_model: Some("Intel(R) Xeon(R) Gold 6338".to_string()),
        disks: vec![
            DiskConfig {
                name: "nvme0n1".to_string(),
                size_gb: 1000,
                disk_type: DiskType::Nvme,
                model: "Samsung 970 EVO Plus 1TB".to_string(),
            },
            DiskConfig {
                name: "sda".to_string(),
                size_gb: 960,
                disk_type: DiskType::Ssd,
                model: "Samsung 860 EVO 960GB".to_string(),
            },
            DiskConfig {
                name: "sdb".to_string(),
                size_gb: 960,
                disk_type: DiskType::Ssd,
                model: "Samsung 860 EVO 960GB".to_string(),
            },
        ],
        nics: vec![
            NicConfig { speed_mbps: 10000 },
            NicConfig { speed_mbps: 10000 },
        ],
    }
}
