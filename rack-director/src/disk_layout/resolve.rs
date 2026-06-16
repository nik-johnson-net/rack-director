use anyhow::{Result, anyhow};
use common::device_attributes::{DeviceAttributes, DiskInfo};
use common::disk_layout::DiskLayout;

use crate::platforms::{self, PlatformAttributes};

/// Error type for label resolution failures, used internally before converting to `anyhow::Error`.
#[derive(Debug)]
enum ResolveError {
    /// The label does not appear in the platform's disk list.
    LabelNotFound(String),
    /// The device has fewer disks than the label's canonical position requires.
    DiskPathMissing(usize),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::LabelNotFound(label) => {
                write!(
                    f,
                    "Platform label '{}' not found in platform attributes",
                    label
                )
            }
            ResolveError::DiskPathMissing(idx) => {
                write!(
                    f,
                    "Device disk at canonical position {} has no by-path device path",
                    idx
                )
            }
        }
    }
}

/// Sort `DiskInfo` slices by the same canonical key used for `PlatformDisk`.
///
/// Disks with unknown type are placed last (priority 4). Disks with unknown size
/// are placed last within their type group.
fn sort_device_disks_canonical(disks: &mut [DiskInfo]) {
    disks.sort_by(|a, b| {
        let priority_a = a.disk_type.map(|t| t.priority()).unwrap_or(4);
        let priority_b = b.disk_type.map(|t| t.priority()).unwrap_or(4);
        priority_a
            .cmp(&priority_b)
            .then_with(|| a.size.unwrap_or(u64::MAX).cmp(&b.size.unwrap_or(u64::MAX)))
    });
}

/// Resolve a disk label to a `by-path` device path using pre-sorted disk slices.
///
/// Resolution order:
/// 1. If the device has an explicit override for this label in
///    `DeviceAttributes.disk_label_overrides`, that path is returned immediately.
/// 2. Otherwise, the label's index in the pre-sorted `sorted_platform_disks` list is used
///    to index into `sorted_device_disks`, and the by-path of the device disk at that
///    index is returned.
///
/// Both `sorted_platform_disks` and `sorted_device_disks` must already be sorted in
/// canonical order (NVMe < SSD < HDD < Unknown, then smaller first) before calling this
/// function.
///
/// Returns `ResolveError::LabelNotFound` if the label does not appear in the platform,
/// or `ResolveError::DiskPathMissing` if the device has fewer disks than needed or the
/// disk at the matching position carries no path.
fn resolve_label(
    label: &str,
    overrides: &std::collections::HashMap<String, String>,
    sorted_platform_disks: &[crate::platforms::PlatformDisk],
    sorted_device_disks: &[DiskInfo],
) -> std::result::Result<String, ResolveError> {
    // Device-level override wins unconditionally.
    if let Some(path) = overrides.get(label) {
        return Ok(path.clone());
    }

    // Canonical position matching: find the label's index in the sorted platform list.
    let idx = sorted_platform_disks
        .iter()
        .position(|d| d.label.as_deref() == Some(label))
        .ok_or_else(|| ResolveError::LabelNotFound(label.to_string()))?;

    // Pick the device disk at the same canonical position.
    sorted_device_disks
        .get(idx)
        .and_then(|d| d.path.clone())
        .ok_or(ResolveError::DiskPathMissing(idx))
}

/// Resolve platform labels in a DiskLayout to actual device paths.
///
/// For each `DiskConfig`, if `device` does not start with `/`, it is treated as a
/// platform label and resolved to a by-path string using `resolve_label`. The same
/// resolution is applied to device references inside ZFS pool definitions.
///
/// Resolution uses:
/// 1. `DeviceAttributes.disk_label_overrides` — operator-specified per-device overrides.
/// 2. Canonical position matching — both platform and device disks are sorted once by
///    `(disk_type_priority, size_gb)`, and the label's position in the platform list
///    is used to select the device disk at the same position.
///
/// Returns a new `DiskLayout` with all labels replaced by device paths, or an error if
/// any label cannot be resolved.
pub fn resolve_disk_layout(
    layout: &DiskLayout,
    platform_attrs: &PlatformAttributes,
    device_attrs: &DeviceAttributes,
) -> Result<DiskLayout> {
    // Pre-sort both disk slices once so that `resolve_label` does not re-sort on
    // every label lookup. This is especially valuable when the layout references
    // multiple labels (e.g. ROOT + DATA1 + DATA2).
    let mut sorted_platform_disks = platform_attrs.disks.clone();
    platforms::sort_disks_canonical(&mut sorted_platform_disks);

    let mut sorted_device_disks = device_attrs.disks.clone();
    sort_device_disks_canonical(&mut sorted_device_disks);

    let overrides = &device_attrs.disk_label_overrides;

    // `wipe_all_disks` and all other scalar fields are preserved by the clone — no
    // explicit copy is needed. If `DiskLayout` gains new fields in the future, verify
    // that this clone still carries them through correctly.
    let mut resolved = layout.clone();

    for disk in &mut resolved.disks {
        if !disk.device.starts_with('/') {
            disk.device = resolve_label(
                &disk.device,
                overrides,
                &sorted_platform_disks,
                &sorted_device_disks,
            )
            .map_err(|e| anyhow!("{}", e))?;
        }
    }

    if let Some(ref mut pools) = resolved.zfs_pools {
        for pool in pools {
            for device_ref in &mut pool.devices {
                if !device_ref.starts_with('/') {
                    *device_ref = resolve_label(
                        device_ref,
                        overrides,
                        &sorted_platform_disks,
                        &sorted_device_disks,
                    )
                    .map_err(|e| anyhow!("{}", e))?;
                }
            }
        }
    }

    Ok(resolved)
}

/// Validate that all labels referenced in a disk layout exist in the platform.
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

/// Check if a disk layout uses any platform labels (device references not starting with '/').
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
    use common::device_attributes::{DiskInfo, DiskType};
    use common::disk_layout::{DiskConfig, PartitionConfig, ZfsPool};
    use std::collections::HashMap;

    // ─────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Platform with three labeled disks: 480 GB SSD (ROOT), 2 TB NVMe (DATA1),
    /// 2 TB NVMe (DATA2). After canonical sort the order is NVMe-2TB, NVMe-2TB,
    /// SSD-480 — but ROOT is on the SSD, so the test platform is intentionally
    /// constructed with labels already set rather than relying on detection.
    ///
    /// For the canonical-position tests we use a simpler platform where ROOT is
    /// unambiguously the smallest/fastest disk.
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

    /// Platform with two labeled disks: 480 GB SSD (ROOT), 2 TB HDD (DATA1).
    /// After canonical sort: [SSD-480, HDD-2000] → ROOT at idx 0, DATA1 at idx 1.
    fn make_simple_platform() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        }
    }

    fn make_disk_info(path: &str, size_gb: u64, disk_type: DiskType) -> DiskInfo {
        DiskInfo {
            name: "disk".to_string(),
            size: Some(size_gb),
            disk_type: Some(disk_type),
            path: Some(path.to_string()),
            model: None,
            serial: None,
            vendor: None,
            uuid: None,
        }
    }

    fn make_device_attrs(disks: Vec<DiskInfo>) -> DeviceAttributes {
        DeviceAttributes {
            disks,
            ..Default::default()
        }
    }

    fn make_device_attrs_with_overrides(
        disks: Vec<DiskInfo>,
        overrides: HashMap<String, String>,
    ) -> DeviceAttributes {
        DeviceAttributes {
            disks,
            disk_label_overrides: overrides,
            ..Default::default()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // resolve_label tests
    // ─────────────────────────────────────────────────────────────────────────

    /// Convenience wrapper matching the old `resolve_label(label, device, platform)` signature.
    ///
    /// Sorts both disk slices and delegates to the production function so that tests
    /// remain readable without repeating the sort boilerplate in every test body.
    fn resolve_label_helper(
        label: &str,
        device: &DeviceAttributes,
        platform: &PlatformAttributes,
    ) -> std::result::Result<String, ResolveError> {
        let mut sorted_platform_disks = platform.disks.clone();
        platforms::sort_disks_canonical(&mut sorted_platform_disks);

        let mut sorted_device_disks = device.disks.clone();
        sort_device_disks_canonical(&mut sorted_device_disks);

        resolve_label(
            label,
            &device.disk_label_overrides,
            &sorted_platform_disks,
            &sorted_device_disks,
        )
    }

    /// A device with identical hardware but different by-path strings should resolve ROOT
    /// to its own disk path, not any path stored on the platform.
    #[test]
    fn test_resolve_label_canonical_position_matching() {
        let platform = make_simple_platform();
        // Device has the same hardware class but different PCI slot addresses.
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);

        let path = resolve_label_helper("ROOT", &device, &platform).unwrap();
        assert_eq!(path, "/dev/disk/by-path/pci-0000:05:00.0-ata-1");

        let path = resolve_label_helper("DATA1", &device, &platform).unwrap();
        assert_eq!(path, "/dev/disk/by-path/pci-0000:06:00.0-ata-2");
    }

    /// When disks arrive from the agent in reverse canonical order, sorting must still
    /// produce the correct label-to-path mapping.
    #[test]
    fn test_resolve_label_device_disks_in_non_canonical_order() {
        let platform = make_simple_platform();
        // Device disks reported with HDD first — sorting must reorder them.
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
        ]);

        let path = resolve_label_helper("ROOT", &device, &platform).unwrap();
        assert_eq!(
            path, "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            "ROOT should map to SSD regardless of disk discovery order"
        );

        let path = resolve_label_helper("DATA1", &device, &platform).unwrap();
        assert_eq!(
            path, "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
            "DATA1 should map to HDD regardless of disk discovery order"
        );
    }

    /// Device override takes precedence over canonical position matching.
    #[test]
    fn test_resolve_label_device_override_wins() {
        let platform = make_simple_platform();
        let mut overrides = HashMap::new();
        overrides.insert(
            "ROOT".to_string(),
            "/dev/disk/by-path/pci-0000:99:00.0-ata-override".to_string(),
        );
        let device = make_device_attrs_with_overrides(
            vec![
                make_disk_info(
                    "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                    480,
                    DiskType::Ssd,
                ),
                make_disk_info(
                    "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                    2000,
                    DiskType::Hdd,
                ),
            ],
            overrides,
        );

        let path = resolve_label_helper("ROOT", &device, &platform).unwrap();
        assert_eq!(
            path, "/dev/disk/by-path/pci-0000:99:00.0-ata-override",
            "Device override must win over canonical resolution"
        );
    }

    /// Override for one label does not affect resolution of other labels.
    #[test]
    fn test_resolve_label_override_only_for_overridden_label() {
        let platform = make_simple_platform();
        let mut overrides = HashMap::new();
        overrides.insert(
            "ROOT".to_string(),
            "/dev/disk/by-path/pci-0000:99:00.0-ata-override".to_string(),
        );
        let device = make_device_attrs_with_overrides(
            vec![
                make_disk_info(
                    "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                    480,
                    DiskType::Ssd,
                ),
                make_disk_info(
                    "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                    2000,
                    DiskType::Hdd,
                ),
            ],
            overrides,
        );

        // DATA1 has no override, so canonical resolution applies.
        let path = resolve_label_helper("DATA1", &device, &platform).unwrap();
        assert_eq!(path, "/dev/disk/by-path/pci-0000:06:00.0-ata-2");
    }

    /// A label that does not appear in the platform returns `LabelNotFound`.
    #[test]
    fn test_resolve_label_not_found_in_platform() {
        let platform = make_simple_platform();
        let device = make_device_attrs(vec![make_disk_info(
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            480,
            DiskType::Ssd,
        )]);

        let result = resolve_label_helper("NONEXISTENT", &device, &platform);
        assert!(
            matches!(result, Err(ResolveError::LabelNotFound(ref l)) if l == "NONEXISTENT"),
            "Expected LabelNotFound, got: {:?}",
            result
        );
    }

    /// When the device has fewer disks than the label's canonical index, resolution
    /// returns `DiskPathMissing`.
    #[test]
    fn test_resolve_label_device_has_fewer_disks_than_platform() {
        let platform = make_simple_platform(); // ROOT=idx0, DATA1=idx1
        // Device only has one disk — DATA1 at index 1 is unreachable.
        let device = make_device_attrs(vec![make_disk_info(
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            480,
            DiskType::Ssd,
        )]);

        let result = resolve_label_helper("DATA1", &device, &platform);
        assert!(
            matches!(result, Err(ResolveError::DiskPathMissing(1))),
            "Expected DiskPathMissing(1), got: {:?}",
            result
        );
    }

    /// When the device disk at the correct canonical index carries no path, resolution
    /// returns `DiskPathMissing`.
    #[test]
    fn test_resolve_label_disk_at_index_has_no_path() {
        let platform = make_simple_platform(); // ROOT=idx0
        let mut disk = make_disk_info(
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            480,
            DiskType::Ssd,
        );
        disk.path = None; // strip the path
        let device = make_device_attrs(vec![disk]);

        let result = resolve_label_helper("ROOT", &device, &platform);
        assert!(
            matches!(result, Err(ResolveError::DiskPathMissing(0))),
            "Expected DiskPathMissing(0), got: {:?}",
            result
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // resolve_disk_layout tests
    // ─────────────────────────────────────────────────────────────────────────

    /// Resolving a layout with an absolute device path (no label) should succeed
    /// unchanged, without requiring device disks.
    #[test]
    fn test_resolve_disk_layout_absolute_path_unchanged() {
        let platform = make_test_platform();
        let device = make_device_attrs(vec![]);
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
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    /// A ROOT label resolves to the correct by-path on the device — not any path
    /// previously associated with the platform.
    #[test]
    fn test_resolve_disk_layout_root_label_resolves_to_device_path() {
        let platform = make_simple_platform();
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1"
        );
    }

    /// Same hardware with different by-path strings (different PCI slot) resolves to
    /// the device's own path, not any path from the platform.
    #[test]
    fn test_resolve_disk_layout_same_hardware_different_paths() {
        let platform = make_simple_platform();
        // Device has same disk specs but different slot addresses than a "canonical" server.
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:0a:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:0b:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert_eq!(
            resolved.disks[0].device, "/dev/disk/by-path/pci-0000:0a:00.0-ata-1",
            "Label should resolve to device's own path, not a path stored on the platform"
        );
    }

    /// A device override takes precedence over canonical resolution in a full layout.
    #[test]
    fn test_resolve_disk_layout_device_override_wins() {
        let platform = make_simple_platform();
        let mut overrides = HashMap::new();
        overrides.insert(
            "ROOT".to_string(),
            "/dev/disk/by-path/pci-0000:99:00.0-override".to_string(),
        );
        let device = make_device_attrs_with_overrides(
            vec![make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            )],
            overrides,
        );
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:99:00.0-override"
        );
    }

    /// A label that does not exist in the platform returns an error mentioning the label.
    #[test]
    fn test_resolve_disk_layout_missing_label() {
        let platform = make_test_platform();
        let device = make_device_attrs(vec![]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "NONEXISTENT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let result = resolve_disk_layout(&layout, &platform, &device);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Platform label 'NONEXISTENT' not found"),
            "Error should identify the missing label"
        );
    }

    /// Device has fewer disks than the label's canonical position requires.
    #[test]
    fn test_resolve_disk_layout_device_disk_path_missing() {
        let platform = make_simple_platform(); // ROOT=idx0, DATA1=idx1
        // Device only has one disk — DATA1 requires index 1.
        let device = make_device_attrs(vec![make_disk_info(
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            480,
            DiskType::Ssd,
        )]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "DATA1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let result = resolve_disk_layout(&layout, &platform, &device);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("canonical position 1"),
            "Error should mention the missing canonical index"
        );
    }

    /// A mixed layout with one label and one absolute path resolves the label and
    /// passes the absolute path through unchanged.
    #[test]
    fn test_resolve_disk_layout_mixed_labels_and_paths() {
        let platform = make_simple_platform();
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);
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
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert_eq!(
            resolved.disks[0].device,
            "/dev/disk/by-path/pci-0000:05:00.0-ata-1"
        );
        assert_eq!(
            resolved.disks[1].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    /// ZFS pool device references that are labels are resolved to by-path strings.
    #[test]
    fn test_resolve_disk_layout_zfs_labels_resolved() {
        let platform = make_simple_platform();
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:05:00.0-ata-1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:06:00.0-ata-2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![],
                },
            ],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "mirror".to_string(),
                devices: vec!["ROOT".to_string(), "DATA1".to_string()],
                datasets: vec![],
                properties: None,
            }]),
            wipe_all_disks: false,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        let pool = &resolved.zfs_pools.unwrap()[0];
        assert_eq!(pool.devices[0], "/dev/disk/by-path/pci-0000:05:00.0-ata-1");
        assert_eq!(pool.devices[1], "/dev/disk/by-path/pci-0000:06:00.0-ata-2");
    }

    /// An empty platform returns a LabelNotFound error when a label is requested.
    #[test]
    fn test_resolve_disk_layout_empty_platform() {
        let empty_platform = PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![],
            memory_gib: 0,
        };
        let device = make_device_attrs(vec![]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let result = resolve_disk_layout(&layout, &empty_platform, &device);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Platform label 'ROOT' not found in platform attributes"),
            "Error message should identify the missing label"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_layout_against_platform tests (unchanged behaviour)
    // ─────────────────────────────────────────────────────────────────────────

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
            wipe_all_disks: false,
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
            wipe_all_disks: false,
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
            wipe_all_disks: false,
        };

        let result = validate_layout_against_platform(&layout, &platform);
        assert!(result.is_ok());
    }

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
            wipe_all_disks: false,
        };

        let result = validate_layout_against_platform(&layout, &platform);
        assert!(result.is_ok());
    }

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
            wipe_all_disks: false,
        };

        let result = validate_layout_against_platform(&layout, &empty_platform);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Disk layout references label 'ROOT'"),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // layout_uses_labels tests
    // ─────────────────────────────────────────────────────────────────────────

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
            wipe_all_disks: false,
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
            wipe_all_disks: false,
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
            wipe_all_disks: false,
        };

        assert!(layout_uses_labels(&layout));
    }

    /// `resolve_disk_layout` must carry `wipe_all_disks: true` through the clone
    /// so the agent receives the flag after label resolution.
    #[test]
    fn test_resolve_disk_layout_preserves_wipe_all_disks() {
        let platform = make_simple_platform();
        let device = make_device_attrs(vec![
            make_disk_info(
                "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
                480,
                DiskType::Ssd,
            ),
            make_disk_info(
                "/dev/disk/by-path/pci-0000:06:00.0-ata-2",
                2000,
                DiskType::Hdd,
            ),
        ]);
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: true,
        };

        let resolved = resolve_disk_layout(&layout, &platform, &device).unwrap();
        assert!(
            resolved.wipe_all_disks,
            "wipe_all_disks must be preserved through resolve_disk_layout"
        );
        assert_eq!(
            resolved.disks[0].device, "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            "ROOT label must still be resolved correctly"
        );
    }
}
