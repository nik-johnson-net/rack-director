use anyhow::{Result, anyhow};
use common::disk_layout::DiskLayout;

use crate::platforms::PlatformAttributes;

/// Resolve platform labels in a DiskLayout to actual device paths.
///
/// For each DiskConfig, if `device` doesn't start with `/`, treat it as a platform label
/// and verify it exists in the platform attributes. Also resolves ZFS pool device references.
///
/// # Note on path resolution (Phase 4 TODO)
///
/// `PlatformDisk` no longer stores a `path` field. Device-path resolution from platform
/// labels to actual by-path strings is now delegated to the agent at provisioning time,
/// where the agent can enumerate the disks present on the specific machine and match them
/// to platform labels by hardware class (disk type + size). Until Phase 4 implements
/// agent-side resolution, this function verifies that all labels exist in the platform but
/// returns an error if any unresolved label is present, because it cannot substitute a
/// path.
///
/// Returns a new DiskLayout with all labels replaced by device paths (if fully resolved),
/// or an error if any label cannot be resolved.
pub fn resolve_disk_layout(
    layout: &DiskLayout,
    platform_attrs: &PlatformAttributes,
) -> Result<DiskLayout> {
    let mut resolved = layout.clone();

    // Verify and resolve disk device labels
    for disk in &mut resolved.disks {
        if !disk.device.starts_with('/') {
            let label = &disk.device;
            let platform_disk = platform_attrs
                .disks
                .iter()
                .find(|d| d.label.as_deref() == Some(label.as_str()))
                .ok_or_else(|| {
                    anyhow!(
                        "Platform label '{}' not found in platform attributes",
                        label
                    )
                })?;
            // TODO(Phase 4): PlatformDisk no longer carries a path. Agent-side label
            // resolution will replace this with the actual by-path device string discovered
            // on the target machine. For now, return an error to prevent silent misresolution.
            return Err(anyhow!(
                "Platform label '{}' cannot be resolved to a device path: \
                 path resolution is not yet implemented (Phase 4). \
                 The platform disk has size_gb={} disk_type={:?}.",
                label,
                platform_disk.size_gb,
                platform_disk.disk_type
            ));
        }
    }

    // Verify and resolve ZFS pool device references
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
                    // TODO(Phase 4): same as above — path not available on PlatformDisk.
                    return Err(anyhow!(
                        "Platform label '{}' in ZFS pool '{}' cannot be resolved to a device path: \
                         path resolution is not yet implemented (Phase 4). \
                         The platform disk has size_gb={} disk_type={:?}.",
                        label,
                        pool.name,
                        platform_disk.size_gb,
                        platform_disk.disk_type
                    ));
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
    use common::disk_layout::{DiskConfig, PartitionConfig, ZfsPool};

    fn make_test_platform() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Nvme,
                    label: Some("DATA1".to_string()),
                },
                PlatformDisk {
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

    /// Resolving a layout with an absolute device path (no label) should succeed unchanged.
    #[test]
    fn test_resolve_disk_layout_absolute_path_unchanged() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
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

        // Absolute path passes through unchanged
        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    /// Resolving a layout that uses a platform label returns a Phase 4 TODO error because
    /// `PlatformDisk` no longer stores a path.
    #[test]
    fn test_resolve_disk_layout_label_returns_phase4_error() {
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

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("ROOT") && err.contains("Phase 4"),
            "Error should mention label 'ROOT' and Phase 4, got: {}",
            err
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

    /// A layout mixing a label and an absolute path fails with the Phase 4 error on the label.
    ///
    /// Absolute paths pass through, but label resolution requires Phase 4 agent-side work.
    #[test]
    fn test_resolve_disk_layout_mixed_labels_and_paths_errors_on_label() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "ROOT".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
            ],
            volume_groups: None,
            zfs_pools: None,
        };

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("ROOT") && err.contains("Phase 4"),
            "Error should mention label 'ROOT' and Phase 4, got: {}",
            err
        );
    }

    /// A ZFS layout that references labels returns the Phase 4 error when the disk device
    /// is a label. Absolute paths in ZFS pool device lists also trigger the Phase 4 error
    /// if disk devices are labels.
    #[test]
    fn test_resolve_disk_layout_with_zfs_label_errors() {
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
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "mirror".to_string(),
                devices: vec!["DATA1".to_string(), "DATA2".to_string()],
                datasets: vec![],
                properties: None,
            }]),
        };

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Phase 4"),
            "Error should mention Phase 4, got: {}",
            err
        );
    }

    /// A ZFS layout where disk devices are absolute paths but pool device references are
    /// labels should still fail with the Phase 4 error on the pool label.
    #[test]
    fn test_resolve_disk_layout_zfs_pool_label_errors() {
        let platform = make_test_platform();
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:03:00.0-nvme-1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:04:00.0-nvme-1".to_string(),
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

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Phase 4") && err.contains("tank"),
            "Error should mention Phase 4 and pool name 'tank', got: {}",
            err
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
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
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
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
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
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
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

    /// Attempting to resolve DATA1 and DATA2 labels returns the Phase 4 error because
    /// `PlatformDisk` no longer stores a path; agent-side resolution is required.
    #[test]
    fn test_resolve_disk_layout_with_data_labels_returns_phase4_error() {
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

        let result = resolve_disk_layout(&layout, &platform);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Phase 4"),
            "Error should mention Phase 4, got: {}",
            err
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
