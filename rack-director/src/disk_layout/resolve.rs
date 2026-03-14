use anyhow::{Result, anyhow};
use common::disk_layout::DiskLayout;

use crate::platforms::PlatformAttributes;

/// Resolve platform labels in a DiskLayout to actual device paths
///
/// For each DiskConfig, if `device` doesn't start with `/`, treat it as a platform label
/// and look up the corresponding disk path from platform attributes.
/// Also resolves ZFS pool device references.
///
/// Returns a new DiskLayout with all labels replaced by device paths.
pub fn resolve_disk_layout(
    layout: &DiskLayout,
    platform_attrs: &PlatformAttributes,
) -> Result<DiskLayout> {
    let mut resolved = layout.clone();

    // Resolve disk device labels
    for disk in &mut resolved.disks {
        if !disk.device.starts_with('/') {
            let label = &disk.device;
            let platform_disk = platform_attrs
                .disks
                .iter()
                .find(|d| d.label.as_deref() == Some(label))
                .ok_or_else(|| {
                    anyhow!(
                        "Platform label '{}' not found in platform attributes",
                        label
                    )
                })?;
            disk.device = platform_disk.path.clone();
        }
    }

    // Resolve ZFS pool device references
    if let Some(ref mut pools) = resolved.zfs_pools {
        for pool in pools {
            for device_ref in &mut pool.devices {
                if !device_ref.starts_with('/') {
                    let label = &*device_ref;
                    let platform_disk = platform_attrs
                        .disks
                        .iter()
                        .find(|d| d.label.as_deref() == Some(label))
                        .ok_or_else(|| {
                            anyhow!(
                                "Platform label '{}' not found in platform attributes (ZFS pool '{}')",
                                label,
                                pool.name
                            )
                        })?;
                    *device_ref = platform_disk.path.clone();
                }
            }
        }
    }

    Ok(resolved)
}

/// Validate that all labels referenced in a disk layout exist in the platform
///
/// Used during role assignment to verify the layout can be satisfied by the platform.
pub fn validate_layout_against_platform(
    layout: &DiskLayout,
    platform_attrs: &PlatformAttributes,
) -> Result<()> {
    // Check disk device labels
    for disk in &layout.disks {
        if !disk.device.starts_with('/') {
            let label = &disk.device;
            if !platform_attrs
                .disks
                .iter()
                .any(|d| d.label.as_deref() == Some(label.as_str()))
            {
                return Err(anyhow!(
                    "Disk layout references label '{}' which does not exist in platform",
                    label
                ));
            }
        }
    }

    // Check ZFS pool device references
    if let Some(ref pools) = layout.zfs_pools {
        for pool in pools {
            for device_ref in &pool.devices {
                if !device_ref.starts_with('/')
                    && !platform_attrs
                        .disks
                        .iter()
                        .any(|d| d.label.as_deref() == Some(device_ref.as_str()))
                {
                    return Err(anyhow!(
                        "ZFS pool '{}' references label '{}' which does not exist in platform",
                        pool.name,
                        device_ref
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Check if a disk layout uses any platform labels (device references not starting with '/')
pub fn layout_uses_labels(layout: &DiskLayout) -> bool {
    // Check disk devices
    if layout.disks.iter().any(|d| !d.device.starts_with('/')) {
        return true;
    }

    // Check ZFS pool devices
    if let Some(ref pools) = layout.zfs_pools
        && pools
            .iter()
            .any(|p| p.devices.iter().any(|d| !d.starts_with('/')))
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::{PlatformCpu, PlatformDisk, PlatformNic};
    use common::device_attributes::DiskType;
    use common::disk_layout::{DiskConfig, PartitionConfig, ZfsDataset, ZfsPool};

    fn make_test_platform() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:03:00.0-nvme-1".to_string(),
                    size_gb: 2000,
                    disk_type: DiskType::Nvme,
                    label: Some("DATA1".to_string()),
                },
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:04:00.0-nvme-1".to_string(),
                    size_gb: 2000,
                    disk_type: DiskType::Nvme,
                    label: Some("DATA2".to_string()),
                },
            ],
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
        }
    }

    #[test]
    fn test_resolve_disk_layout_success() {
        let platform = make_test_platform();
        let layout = DiskLayout {
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
        };

        let resolved = resolve_disk_layout(&layout, &platform).unwrap();

        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    #[test]
    fn test_resolve_disk_layout_missing_label() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "NONEXISTENT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Platform label 'NONEXISTENT' not found")
        );
    }

    #[test]
    fn test_resolve_disk_layout_mixed_labels_and_paths() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "ROOT".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "/dev/sda".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
            ],
            volume_groups: None,
            zfs_pools: None,
        };

        let resolved = resolve_disk_layout(&layout, &platform).unwrap();

        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
        assert_eq!(resolved.disks[1].device, "/dev/sda");
    }

    #[test]
    fn test_resolve_disk_layout_with_zfs() {
        let platform = make_test_platform();
        let _layout = DiskLayout {
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
                datasets: vec![ZfsDataset {
                    name: "tank/data".to_string(),
                    mount_point: Some("/data".to_string()),
                    properties: None,
                    zvol_size: None,
                }],
                properties: None,
            }]),
        };

        // This should fail because DATA1-zfs1 is a partition reference, not a label
        // Let me create a test with actual labels
        let layout_with_labels = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "DATA1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "DATA2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
            ],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "mirror".to_string(),
                devices: vec!["DATA1".to_string(), "DATA2".to_string()],
                datasets: vec![],
                properties: None,
            }]),
        };

        let resolved = resolve_disk_layout(&layout_with_labels, &platform).unwrap();

        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:03:00.0-nvme-1"
        );
        assert_eq!(
            resolved.disks[1].device,
            "/dev/disk/by-path/pci-0000:04:00.0-nvme-1"
        );
        assert_eq!(
            resolved.zfs_pools.as_ref().unwrap()[0].devices[0],
            "/dev/disk/by-path/pci-0000:03:00.0-nvme-1"
        );
        assert_eq!(
            resolved.zfs_pools.as_ref().unwrap()[0].devices[1],
            "/dev/disk/by-path/pci-0000:04:00.0-nvme-1"
        );
    }

    #[test]
    fn test_validate_layout_against_platform_success() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = validate_layout_against_platform(&layout, &platform);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_layout_against_platform_missing_label() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "MISSING".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = validate_layout_against_platform(&layout, &platform);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Disk layout references label 'MISSING'")
        );
    }

    #[test]
    fn test_validate_layout_against_platform_with_paths() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        // Should succeed - absolute paths don't require validation
        let result = validate_layout_against_platform(&layout, &platform);
        assert!(result.is_ok());
    }

    #[test]
    fn test_layout_uses_labels_true() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        assert!(layout_uses_labels(&layout));
    }

    #[test]
    fn test_layout_uses_labels_false() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        assert!(!layout_uses_labels(&layout));
    }

    #[test]
    fn test_layout_uses_labels_zfs() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "single".to_string(),
                devices: vec!["DATA1".to_string()],
                datasets: vec![],
                properties: None,
            }]),
        };

        assert!(layout_uses_labels(&layout));
    }

    /// Resolve a layout that uses DATA1 and DATA2 labels against a platform that has those labels.
    /// Verifies that each label resolves to its correct device path.
    #[test]
    fn test_resolve_disk_layout_with_data_labels() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "DATA1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "data1".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("xfs".to_string()),
                        mount_point: Some("/data1".to_string()),
                        flags: None,
                        volume_group: None,
                    }],
                },
                DiskConfig {
                    device: "DATA2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "data2".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("xfs".to_string()),
                        mount_point: Some("/data2".to_string()),
                        flags: None,
                        volume_group: None,
                    }],
                },
            ],
            volume_groups: None,
            zfs_pools: None,
        };

        let resolved = resolve_disk_layout(&layout, &platform).unwrap();

        assert_eq!(
            resolved.disks[0].device, "/dev/disk/by-path/pci-0000:03:00.0-nvme-1",
            "DATA1 should resolve to the first NVMe path"
        );
        assert_eq!(
            resolved.disks[1].device, "/dev/disk/by-path/pci-0000:04:00.0-nvme-1",
            "DATA2 should resolve to the second NVMe path"
        );
    }

    /// Attempt to resolve a layout with a ROOT label against a platform that has no labels at all.
    /// Should return an error because the label cannot be found.
    #[test]
    fn test_resolve_disk_layout_empty_platform() {
        let empty_platform = PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![],
            memory_gib: 0,
        };
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = resolve_disk_layout(&layout, &empty_platform);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Platform label 'ROOT' not found in platform attributes"),
            "Error message should identify the missing label"
        );
    }

    /// Validate a layout using DATA1 and DATA2 labels against a platform that has those labels.
    /// Should succeed because all referenced labels are present.
    #[test]
    fn test_validate_layout_against_platform_data_labels() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "DATA1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "DATA2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
            ],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = validate_layout_against_platform(&layout, &platform);

        assert!(
            result.is_ok(),
            "Validation should succeed when DATA1 and DATA2 are present in platform"
        );
    }

    /// Validate a layout with a ROOT label against a platform that has no labels.
    /// Should return an error because the required label is absent.
    #[test]
    fn test_validate_layout_against_platform_empty_platform() {
        let empty_platform = PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![],
            memory_gib: 0,
        };
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = validate_layout_against_platform(&layout, &empty_platform);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Disk layout references label 'ROOT'"),
            "Error message should identify the missing label"
        );
    }
}
