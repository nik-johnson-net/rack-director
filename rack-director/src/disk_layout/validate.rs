use std::collections::HashMap;

use common::disk_layout::DiskLayout;

use super::layout_uses_labels;

const VALID_FILESYSTEMS: &[&str] = &["ext4", "xfs", "btrfs", "vfat", "swap"];
const VALID_PARTITION_TABLES: &[&str] = &["gpt", "msdos"];

/// Validate a complete disk layout for structural correctness.
///
/// All errors are collected before returning — this function does not short-circuit on the
/// first failure. Error keys use dot-path format that matches the frontend field names, e.g.
/// `"disks.0.partitions.1.size"`.
///
/// Returns `Ok(())` when the layout is valid, or `Err(HashMap<String, String>)` containing
/// all detected validation errors keyed by field path.
pub fn validate_disk_layout(layout: &DiskLayout) -> Result<(), HashMap<String, String>> {
    let mut errors = HashMap::new();

    validate_at_least_one_disk(layout, &mut errors);
    validate_root_disk_required(layout, &mut errors);
    validate_disks(layout, &mut errors);
    validate_volume_groups(layout, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_at_least_one_disk(layout: &DiskLayout, errors: &mut HashMap<String, String>) {
    if layout.disks.is_empty() {
        errors.insert(
            "disk_layout".to_string(),
            "At least one disk is required".to_string(),
        );
    }
}

/// Check that a disk with label "ROOT" exists when the layout uses labels.
fn validate_root_disk_required(layout: &DiskLayout, errors: &mut HashMap<String, String>) {
    if layout.disks.is_empty() {
        // Already reported above; no point adding a second error for the same root cause.
        return;
    }
    if layout_uses_labels(layout) {
        let has_root = layout.disks.iter().any(|d| d.device == "ROOT");
        if !has_root {
            errors.insert(
                "disk_layout".to_string(),
                "A disk with label 'ROOT' is required".to_string(),
            );
        }
    }
}

fn validate_disks(layout: &DiskLayout, errors: &mut HashMap<String, String>) {
    // Collect the set of volume group names defined in the layout for cross-reference checks.
    let defined_vg_names: std::collections::HashSet<&str> = layout
        .volume_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|vg| vg.name.as_str())
        .collect();

    for (i, disk) in layout.disks.iter().enumerate() {
        validate_partition_table(i, &disk.partition_table, errors);
        validate_disk_partitions(i, &disk.partitions, &defined_vg_names, errors);
    }
}

fn validate_partition_table(
    disk_index: usize,
    partition_table: &str,
    errors: &mut HashMap<String, String>,
) {
    if !VALID_PARTITION_TABLES.contains(&partition_table) {
        errors.insert(
            format!("disks.{}.partition_table", disk_index),
            "Partition table must be 'gpt' or 'msdos'".to_string(),
        );
    }
}

fn validate_disk_partitions(
    disk_index: usize,
    partitions: &[common::disk_layout::PartitionConfig],
    defined_vg_names: &std::collections::HashSet<&str>,
    errors: &mut HashMap<String, String>,
) {
    validate_at_most_one_rest_partition(disk_index, partitions, errors);

    for (j, partition) in partitions.iter().enumerate() {
        validate_partition_filesystem(disk_index, j, partition.filesystem.as_deref(), errors);
        validate_lvm_partition(disk_index, j, partition, defined_vg_names, errors);
    }
}

/// Ensure that no more than one partition per disk uses "rest" or "*" as its size.
///
/// When multiple offending partitions are found, each one is reported individually using
/// its own `disks.N.partitions.J.size` key so the frontend can highlight the specific
/// field that needs to be changed.
fn validate_at_most_one_rest_partition(
    disk_index: usize,
    partitions: &[common::disk_layout::PartitionConfig],
    errors: &mut HashMap<String, String>,
) {
    let rest_indices: Vec<usize> = partitions
        .iter()
        .enumerate()
        .filter(|(_, p)| p.size == "rest" || p.size == "*")
        .map(|(i, _)| i)
        .collect();

    if rest_indices.len() > 1 {
        for &j in &rest_indices {
            errors.insert(
                format!("disks.{}.partitions.{}.size", disk_index, j),
                "Only one partition per disk can use 'rest' or '*' for size".to_string(),
            );
        }
    }
}

/// Validate the filesystem type for a regular (non-LVM) partition.
fn validate_partition_filesystem(
    disk_index: usize,
    partition_index: usize,
    filesystem: Option<&str>,
    errors: &mut HashMap<String, String>,
) {
    if let Some(fs) = filesystem
        && !VALID_FILESYSTEMS.contains(&fs)
    {
        errors.insert(
            format!(
                "disks.{}.partitions.{}.filesystem",
                disk_index, partition_index
            ),
            format!(
                "Invalid filesystem '{}'. Valid options: ext4, xfs, btrfs, vfat, swap",
                fs
            ),
        );
    }
}

/// Validate LVM-related fields on a partition.
///
/// - A partition whose flags contain "lvm" must specify a volume_group.
/// - Any volume_group reference must point to a defined VolumeGroup.
fn validate_lvm_partition(
    disk_index: usize,
    partition_index: usize,
    partition: &common::disk_layout::PartitionConfig,
    defined_vg_names: &std::collections::HashSet<&str>,
    errors: &mut HashMap<String, String>,
) {
    let is_lvm = partition
        .flags
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|f| f == "lvm");

    if is_lvm {
        match &partition.volume_group {
            None => {
                errors.insert(
                    format!(
                        "disks.{}.partitions.{}.volume_group",
                        disk_index, partition_index
                    ),
                    "LVM partition must be assigned to a volume group".to_string(),
                );
            }
            Some(vg_name) => {
                if !defined_vg_names.contains(vg_name.as_str()) {
                    errors.insert(
                        format!(
                            "disks.{}.partitions.{}.volume_group",
                            disk_index, partition_index
                        ),
                        format!("Volume group '{}' does not exist", vg_name),
                    );
                }
            }
        }
    }
}

/// Validate all volume groups: check filesystems on logical volumes and that each VG is
/// referenced by at least one partition.
fn validate_volume_groups(layout: &DiskLayout, errors: &mut HashMap<String, String>) {
    let Some(ref volume_groups) = layout.volume_groups else {
        return;
    };

    for (i, vg) in volume_groups.iter().enumerate() {
        validate_vg_has_physical_volumes(i, vg, layout, errors);
        validate_logical_volume_filesystems(i, vg, errors);
    }
}

/// Ensure each VolumeGroup is referenced by at least one partition with a matching
/// `volume_group` field.
fn validate_vg_has_physical_volumes(
    vg_index: usize,
    vg: &common::disk_layout::VolumeGroup,
    layout: &DiskLayout,
    errors: &mut HashMap<String, String>,
) {
    let referenced = layout.disks.iter().any(|disk| {
        disk.partitions
            .iter()
            .any(|p| p.volume_group.as_deref() == Some(vg.name.as_str()))
    });

    if !referenced {
        errors.insert(
            format!("volume_groups.{}.name", vg_index),
            format!(
                "Volume group '{}' has no physical volumes assigned",
                vg.name
            ),
        );
    }
}

/// Validate that each logical volume in a VG has a valid, recognised filesystem.
fn validate_logical_volume_filesystems(
    vg_index: usize,
    vg: &common::disk_layout::VolumeGroup,
    errors: &mut HashMap<String, String>,
) {
    for (j, lv) in vg.logical_volumes.iter().enumerate() {
        if !VALID_FILESYSTEMS.contains(&lv.filesystem.as_str()) {
            errors.insert(
                format!(
                    "volume_groups.{}.logical_volumes.{}.filesystem",
                    vg_index, j
                ),
                format!(
                    "Invalid filesystem '{}'. Valid options: ext4, xfs, btrfs, vfat, swap",
                    lv.filesystem
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::disk_layout::{
        DiskConfig, DiskLayout, LogicalVolume, PartitionConfig, VolumeGroup,
    };

    // ===== Helpers =====

    fn simple_layout_with_root() -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    fn simple_layout_with_path() -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    // ===== at least one disk =====

    #[test]
    fn test_empty_disks_returns_error() {
        let layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.contains_key("disk_layout"),
            "expected disk_layout key"
        );
        assert!(errors["disk_layout"].contains("At least one disk"));
    }

    // ===== ROOT disk required =====

    #[test]
    fn test_label_layout_without_root_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "DATA1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "data".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("xfs".to_string()),
                    mount_point: Some("/data".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("disk_layout"));
        assert!(errors["disk_layout"].contains("ROOT"));
    }

    #[test]
    fn test_label_layout_with_root_is_ok() {
        let layout = simple_layout_with_root();
        assert!(validate_disk_layout(&layout).is_ok());
    }

    #[test]
    fn test_path_layout_without_root_label_is_ok() {
        // Layouts that use absolute paths do not require a ROOT label.
        let layout = simple_layout_with_path();
        assert!(validate_disk_layout(&layout).is_ok());
    }

    // ===== partition table =====

    #[test]
    fn test_invalid_partition_table_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "mbr".to_string(), // invalid
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("disks.0.partition_table"));
        assert!(errors["disks.0.partition_table"].contains("gpt"));
    }

    #[test]
    fn test_valid_partition_tables() {
        for table in ["gpt", "msdos"] {
            let layout = DiskLayout {
                disks: vec![DiskConfig {
                    device: "/dev/sda".to_string(),
                    partition_table: table.to_string(),
                    partitions: vec![PartitionConfig {
                        label: "root".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/".to_string()),
                        flags: None,
                        volume_group: None,
                    }],
                }],
                volume_groups: None,
                zfs_pools: None,
            };
            assert!(
                validate_disk_layout(&layout).is_ok(),
                "partition_table '{}' should be valid",
                table
            );
        }
    }

    // ===== at most one "rest" partition =====

    #[test]
    fn test_multiple_rest_partitions_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "p1".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "p2".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("xfs".to_string()),
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    },
                ],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Each offending partition gets its own per-field error key.
        assert!(
            errors.contains_key("disks.0.partitions.0.size"),
            "expected size error for partition 0, got: {:?}",
            errors
        );
        assert!(
            errors.contains_key("disks.0.partitions.1.size"),
            "expected size error for partition 1, got: {:?}",
            errors
        );
        assert!(errors["disks.0.partitions.0.size"].contains("Only one"));
    }

    #[test]
    fn test_multiple_star_partitions_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "p1".to_string(),
                        size: "*".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "p2".to_string(),
                        size: "*".to_string(),
                        filesystem: Some("xfs".to_string()),
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    },
                ],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Both partitions with "*" size should be flagged individually.
        assert!(
            errors.contains_key("disks.0.partitions.0.size"),
            "expected size error for partition 0, got: {:?}",
            errors
        );
        assert!(
            errors.contains_key("disks.0.partitions.1.size"),
            "expected size error for partition 1, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_one_rest_partition_is_ok() {
        let layout = simple_layout_with_path();
        assert!(validate_disk_layout(&layout).is_ok());
    }

    // ===== partition filesystem =====

    #[test]
    fn test_invalid_partition_filesystem_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ntfs".to_string()), // not in valid set
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("disks.0.partitions.0.filesystem"));
        assert!(errors["disks.0.partitions.0.filesystem"].contains("ntfs"));
    }

    #[test]
    fn test_valid_partition_filesystems() {
        for fs in ["ext4", "xfs", "btrfs", "vfat", "swap"] {
            let layout = DiskLayout {
                disks: vec![DiskConfig {
                    device: "/dev/sda".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "p".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some(fs.to_string()),
                        mount_point: None,
                        flags: None,
                        volume_group: None,
                    }],
                }],
                volume_groups: None,
                zfs_pools: None,
            };
            assert!(
                validate_disk_layout(&layout).is_ok(),
                "filesystem '{}' should be valid",
                fs
            );
        }
    }

    #[test]
    fn test_none_filesystem_is_ok() {
        // Partitions without filesystem (LVM/ZFS raw) are allowed to have no filesystem.
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: Some(vec!["lvm".to_string()]),
                    volume_group: Some("vg0".to_string()),
                }],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![LogicalVolume {
                    name: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: "ext4".to_string(),
                    mount_point: Some("/".to_string()),
                }],
            }]),
            zfs_pools: None,
        };
        assert!(validate_disk_layout(&layout).is_ok());
    }

    // ===== LVM partition must have volume_group =====

    #[test]
    fn test_lvm_partition_without_volume_group_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: Some(vec!["lvm".to_string()]),
                    volume_group: None, // missing!
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("disks.0.partitions.0.volume_group"));
        assert!(errors["disks.0.partitions.0.volume_group"].contains("LVM partition"));
    }

    #[test]
    fn test_lvm_partition_with_nonexistent_vg_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: Some(vec!["lvm".to_string()]),
                    volume_group: Some("nonexistent_vg".to_string()),
                }],
            }],
            volume_groups: None, // no VGs defined!
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("disks.0.partitions.0.volume_group"));
        assert!(errors["disks.0.partitions.0.volume_group"].contains("nonexistent_vg"));
    }

    // ===== volume_group unreferenced =====

    #[test]
    fn test_unreferenced_volume_group_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "orphan_vg".to_string(),
                logical_volumes: vec![LogicalVolume {
                    name: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: "ext4".to_string(),
                    mount_point: Some("/".to_string()),
                }],
            }]),
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.contains_key("volume_groups.0.name"),
            "expected volume_groups.0.name key, got: {:?}",
            errors
        );
        assert!(errors["volume_groups.0.name"].contains("orphan_vg"));
    }

    // ===== logical volume filesystems =====

    #[test]
    fn test_invalid_lv_filesystem_returns_error() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: Some(vec!["lvm".to_string()]),
                    volume_group: Some("vg0".to_string()),
                }],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![LogicalVolume {
                    name: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: "fat32".to_string(), // invalid
                    mount_point: Some("/".to_string()),
                }],
            }]),
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("volume_groups.0.logical_volumes.0.filesystem"));
        assert!(errors["volume_groups.0.logical_volumes.0.filesystem"].contains("fat32"));
    }

    #[test]
    fn test_valid_lvm_layout_is_ok() {
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
                ],
            }]),
            zfs_pools: None,
        };
        assert!(validate_disk_layout(&layout).is_ok());
    }

    // ===== all errors collected =====

    #[test]
    fn test_multiple_errors_all_returned() {
        // Layout with both an invalid partition table and an invalid filesystem.
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "invalid_table".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ntfs".to_string()),
                    mount_point: None,
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let result = validate_disk_layout(&layout);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Both errors must be present.
        assert!(
            errors.contains_key("disks.0.partition_table"),
            "expected partition_table error, got: {:?}",
            errors
        );
        assert!(
            errors.contains_key("disks.0.partitions.0.filesystem"),
            "expected filesystem error, got: {:?}",
            errors
        );
    }

    // ===== valid complex layout =====

    #[test]
    fn test_valid_uefi_layout_is_ok() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "efi".to_string(),
                        size: "300MiB".to_string(),
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
        assert!(validate_disk_layout(&layout).is_ok());
    }
}
