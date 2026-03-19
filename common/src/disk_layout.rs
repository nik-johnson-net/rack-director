use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete disk layout configuration for a device
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiskLayout {
    pub disks: Vec<DiskConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_groups: Option<Vec<VolumeGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zfs_pools: Option<Vec<ZfsPool>>,
}

/// Configuration for a single disk
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiskConfig {
    pub device: String, // Platform label ("ROOT") or path ("/dev/sda")
    #[serde(default = "default_partition_table")]
    pub partition_table: String, // "gpt" (default) or "msdos"
    pub partitions: Vec<PartitionConfig>,
}

fn default_partition_table() -> String {
    "gpt".to_string()
}

/// Configuration for a single partition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartitionConfig {
    pub label: String, // GPT partition name
    pub size: String,  // "512MiB", "50%", "rest"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<String>, // None if used by LVM/ZFS
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<Vec<String>>, // "boot", "esp", "lvm"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_group: Option<String>, // LVM VG this partition joins
}

/// LVM Volume Group configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeGroup {
    pub name: String,
    pub logical_volumes: Vec<LogicalVolume>,
}

/// LVM Logical Volume configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogicalVolume {
    pub name: String,
    pub size: String, // "50G", "100%FREE"
    pub filesystem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
}

/// ZFS Pool configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZfsPool {
    pub name: String,
    pub vdev_type: String,    // "single", "mirror", "raidz", "raidz2"
    pub devices: Vec<String>, // Partition refs or device paths
    pub datasets: Vec<ZfsDataset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

/// ZFS Dataset configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZfsDataset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zvol_size: Option<String>, // Creates zvol if set
}

/// Generate partition device path from a disk path and partition number.
///
/// For SATA/SCSI: /dev/sda + 1 = /dev/sda1
/// For NVMe: /dev/nvme0n1 + 1 = /dev/nvme0n1p1
/// For device-mapper: /dev/dm-0 + 1 = /dev/dm-0p1
/// For by-path/by-id symlinks: /dev/disk/by-path/pci-0000:00:03.0 + 1 = /dev/disk/by-path/pci-0000:00:03.0-part1
pub fn partition_path(disk: &str, partition_num: usize) -> String {
    if disk.contains("/by-path/") || disk.contains("/by-id/") {
        return format!("{}-part{}", disk, partition_num);
    }
    let needs_p = disk.chars().last().is_some_and(|c| c.is_ascii_digit());
    if needs_p {
        format!("{}p{}", disk, partition_num)
    } else {
        format!("{}{}", disk, partition_num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== partition_path tests ==========

    #[test]
    fn test_partition_path_sata() {
        assert_eq!(partition_path("/dev/sda", 1), "/dev/sda1");
        assert_eq!(partition_path("/dev/sda", 2), "/dev/sda2");
        assert_eq!(partition_path("/dev/sdb", 10), "/dev/sdb10");
    }

    #[test]
    fn test_partition_path_nvme() {
        assert_eq!(partition_path("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
        assert_eq!(partition_path("/dev/nvme0n1", 2), "/dev/nvme0n1p2");
        assert_eq!(partition_path("/dev/nvme1n1", 3), "/dev/nvme1n1p3");
    }

    #[test]
    fn test_partition_path_dm() {
        assert_eq!(partition_path("/dev/dm-0", 1), "/dev/dm-0p1");
    }

    #[test]
    fn test_partition_path_by_path() {
        assert_eq!(
            partition_path("/dev/disk/by-path/pci-0000:00:03.0", 1),
            "/dev/disk/by-path/pci-0000:00:03.0-part1"
        );
        assert_eq!(
            partition_path("/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio1", 2),
            "/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio1-part2"
        );
    }

    #[test]
    fn test_partition_path_by_id() {
        assert_eq!(
            partition_path("/dev/disk/by-id/wwn-0x5000c500-0", 1),
            "/dev/disk/by-id/wwn-0x5000c500-0-part1"
        );
    }

    #[test]
    fn test_simple_layout_serialization_roundtrip() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "boot".to_string(),
                        size: "512MiB".to_string(),
                        filesystem: Some("vfat".to_string()),
                        mount_point: Some("/boot/efi".to_string()),
                        flags: Some(vec!["esp".to_string()]),
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/".to_string()),
                        flags: None,
                        volume_group: None,
                    },
                ],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let json = serde_json::to_string(&layout).unwrap();
        let deserialized: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_lvm_layout_serialization_roundtrip() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "boot".to_string(),
                        size: "1GiB".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/boot".to_string()),
                        flags: Some(vec!["boot".to_string()]),
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "lvm".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: Some(vec!["lvm".to_string()]),
                        volume_group: Some("vg0".to_string()),
                    },
                ],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![
                    LogicalVolume {
                        name: "root".to_string(),
                        size: "50G".to_string(),
                        filesystem: "ext4".to_string(),
                        mount_point: Some("/".to_string()),
                    },
                    LogicalVolume {
                        name: "swap".to_string(),
                        size: "8G".to_string(),
                        filesystem: "swap".to_string(),
                        mount_point: None,
                    },
                    LogicalVolume {
                        name: "home".to_string(),
                        size: "100%FREE".to_string(),
                        filesystem: "ext4".to_string(),
                        mount_point: Some("/home".to_string()),
                    },
                ],
            }]),
            zfs_pools: None,
        };

        let json = serde_json::to_string(&layout).unwrap();
        let deserialized: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_zfs_layout_serialization_roundtrip() {
        let mut pool_properties = HashMap::new();
        pool_properties.insert("ashift".to_string(), "12".to_string());

        let mut dataset_properties = HashMap::new();
        dataset_properties.insert("compression".to_string(), "lz4".to_string());
        dataset_properties.insert("atime".to_string(), "off".to_string());

        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "DATA1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "zfs1".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    }],
                },
                DiskConfig {
                    device: "DATA2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "zfs2".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    }],
                },
            ],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "mirror".to_string(),
                devices: vec!["DATA1-zfs1".to_string(), "DATA2-zfs2".to_string()],
                datasets: vec![
                    ZfsDataset {
                        name: "tank/data".to_string(),
                        mount_point: Some("/data".to_string()),
                        properties: Some(dataset_properties.clone()),
                        zvol_size: None,
                    },
                    ZfsDataset {
                        name: "tank/swap".to_string(),
                        mount_point: None,
                        properties: None,
                        zvol_size: Some("8G".to_string()),
                    },
                ],
                properties: Some(pool_properties),
            }]),
        };

        let json = serde_json::to_string(&layout).unwrap();
        let deserialized: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_default_partition_table_is_gpt() {
        // Test that omitting partition_table in JSON defaults to "gpt"
        let json = r#"{
            "disks": [{
                "device": "/dev/sda",
                "partitions": []
            }],
            "volume_groups": null,
            "zfs_pools": null
        }"#;

        let layout: DiskLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.disks[0].partition_table, "gpt");
    }

    #[test]
    fn test_optional_fields_omit_from_json() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let json = serde_json::to_string(&layout).unwrap();

        // volume_groups and zfs_pools should not appear in JSON when None
        assert!(!json.contains("volume_groups"));
        assert!(!json.contains("zfs_pools"));

        // Verify it round-trips correctly
        let deserialized: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_partition_optional_fields_omit_from_json() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let json = serde_json::to_string(&layout).unwrap();
        let partition_json = &json[json.find("\"partitions\"").unwrap()..];

        // filesystem, mount_point, flags, volume_group should not appear when None
        let first_brace = partition_json.find('{').unwrap();
        let last_brace = partition_json.find('}').unwrap();
        let partition_obj = &partition_json[first_brace..=last_brace];

        assert!(!partition_obj.contains("\"filesystem\""));
        assert!(!partition_obj.contains("\"mount_point\""));
        assert!(!partition_obj.contains("\"flags\""));
        assert!(!partition_obj.contains("\"volume_group\""));

        // Verify it round-trips correctly
        let deserialized: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout, deserialized);
    }
}
