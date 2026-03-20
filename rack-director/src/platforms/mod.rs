mod cpu_model;
mod detection;
pub mod store;

pub use detection::detect_or_create_platform;
pub(crate) use detection::sort_disks_canonical;

// Re-export types for convenience
pub use common::device_attributes::DiskType;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::FromRow;

/// A Platform defines a common hardware configuration for similar physical devices.
/// Platforms provide labels (ROOT, DATA1, NIC1) that Roles can reference in disk layouts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub attributes: PlatformAttributes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl FromRow for Platform {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let attributes_json: String = row.get("attributes")?;
        let attributes: PlatformAttributes =
            serde_json::from_str(&attributes_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    attributes_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        Ok(Platform {
            id: row.get("id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            attributes,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// Platform hardware attributes defining the expected configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformAttributes {
    pub disks: Vec<PlatformDisk>,
    pub nics: Vec<PlatformNic>,
    pub cpus: Vec<PlatformCpu>,
    pub memory_gib: u32,
}

/// Platform disk specification with label.
///
/// Describes disk hardware class only — no device path is stored. Paths vary by PCIe bus
/// topology, so two identical servers in different physical slots would otherwise resolve
/// labels to the wrong disks. Label resolution to actual device paths is performed at
/// provisioning time by the agent (Phase 4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformDisk {
    /// Disk size in GB
    pub size_gb: u64,
    /// Disk type (nvme, ssd, hdd)
    pub disk_type: DiskType,
    /// Optional label for role reference (ROOT, DATA1, DATA2, etc.)
    pub label: Option<String>,
}

/// Platform NIC specification with label
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformNic {
    /// Logical interface name (e.g., "eno1", "eth0")
    pub logical: String,
    /// Link speed in Mbps (e.g., 1000, 10000, 25000)
    pub speed_mbps: Option<u32>,
    /// Optional label for role reference (NIC1, NIC2, etc.)
    pub label: Option<String>,
}

/// Platform CPU specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformCpu {
    /// CPU brand/manufacturer (intel, amd)
    pub brand: String,
    /// CPU model string (e.g., "E3-1240 v3", "Xeon Gold 6248R")
    pub model: String,
    /// Number of physical cores
    pub cores: u32,
}

/// Request to create a new platform
#[derive(Debug, Deserialize)]
pub struct CreatePlatformRequest {
    pub name: String,
    pub description: Option<String>,
    pub attributes: PlatformAttributes,
}

/// Request to update an existing platform
#[derive(Debug, Deserialize)]
pub struct UpdatePlatformRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub attributes: Option<PlatformAttributes>,
}

/// Request to assign a platform to a device
#[derive(Debug, Deserialize)]
pub struct AssignPlatformRequest {
    pub platform_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_attributes_serialization() {
        let attrs = PlatformAttributes {
            disks: vec![PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![PlatformNic {
                logical: "eno1".to_string(),
                speed_mbps: Some(10000),
                label: Some("NIC1".to_string()),
            }],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        };

        // Serialize to JSON
        let json_str = serde_json::to_string(&attrs).unwrap();

        // Deserialize back
        let deserialized: PlatformAttributes = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized, attrs);
    }

    /// Old JSON that still contains a `path` field must deserialize without error.
    ///
    /// Serde ignores unknown fields by default, so existing database rows that were
    /// created before `path` was removed will continue to load cleanly.
    #[test]
    fn test_platform_disk_deserializes_with_legacy_path_field() {
        let legacy_json = r#"{
            "disks": [
                {
                    "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
                    "size_gb": 480,
                    "disk_type": "ssd",
                    "label": "ROOT"
                }
            ],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        }"#;

        let attrs: PlatformAttributes = serde_json::from_str(legacy_json).unwrap();

        assert_eq!(attrs.disks.len(), 1);
        assert_eq!(attrs.disks[0].size_gb, 480);
        assert_eq!(attrs.disks[0].disk_type, DiskType::Ssd);
        assert_eq!(attrs.disks[0].label, Some("ROOT".to_string()));
    }
}
