use common::{
    FirmwareMode,
    disk_layout::{DiskConfig, DiskLayout, PartitionConfig},
};

/// Inject a firmware-appropriate boot partition into the ROOT disk when one is not already present.
///
/// This must be called BEFORE label resolution, while the ROOT disk still has `device == "ROOT"`.
/// Only applies to layouts that use platform labels (i.e., when `device == "ROOT"` exists).
///
/// For path-based layouts (no labels), no injection is performed — the operator is expected
/// to include boot partitions explicitly.
///
/// Injection rules (only when `boot_mode` is `Some`):
/// - UEFI: prepend a 300 MiB vfat partition with the `esp` flag and `/boot/efi` mount point
/// - BIOS + GPT: prepend a 1 MiB partition with the `bios_grub` flag (no filesystem)
/// - BIOS + MBR or `None` boot_mode: no injection
pub fn inject_boot_partition(layout: &mut DiskLayout, boot_mode: Option<FirmwareMode>) {
    let Some(mode) = boot_mode else {
        return;
    };

    // Find the ROOT disk by label. This function must be called before label resolution,
    // so "ROOT" is still the device field value rather than an actual path.
    // Path-based layouts (device = "/dev/...") do not have a ROOT label and are skipped.
    let Some(root_disk) = layout.disks.iter_mut().find(|d| d.device == "ROOT") else {
        return;
    };

    match mode {
        FirmwareMode::Uefi => inject_esp_partition(root_disk),
        FirmwareMode::Bios => inject_bios_grub_partition(root_disk),
    }
}

/// Prepend a 300 MiB EFI System Partition to `disk` if no partition already has the `esp` flag.
fn inject_esp_partition(disk: &mut DiskConfig) {
    let already_has_esp = disk.partitions.iter().any(|p| {
        p.flags
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|f| f == "esp")
    });

    if already_has_esp {
        return;
    }

    let esp = PartitionConfig {
        label: "efi".to_string(),
        size: "300MiB".to_string(),
        filesystem: Some("vfat".to_string()),
        mount_point: Some("/boot/efi".to_string()),
        flags: Some(vec!["esp".to_string()]),
        volume_group: None,
    };

    disk.partitions.insert(0, esp);
}

/// Prepend a 1 MiB BIOS GRUB partition to `disk` when the partition table is `gpt` and no
/// partition already has the `bios_grub` flag.
fn inject_bios_grub_partition(disk: &mut DiskConfig) {
    if disk.partition_table != "gpt" {
        return;
    }

    let already_has_bios_grub = disk.partitions.iter().any(|p| {
        p.flags
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|f| f == "bios_grub")
    });

    if already_has_bios_grub {
        return;
    }

    let bios_grub = PartitionConfig {
        label: "bios_grub".to_string(),
        size: "1MiB".to_string(),
        filesystem: None,
        mount_point: None,
        flags: Some(vec!["bios_grub".to_string()]),
        volume_group: None,
    };

    disk.partitions.insert(0, bios_grub);
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::disk_layout::DiskLayout;

    // ========== helpers ==========

    fn root_labeled_disk_gpt(partitions: Vec<PartitionConfig>) -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions,
            }],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    fn root_partition() -> PartitionConfig {
        PartitionConfig {
            label: "root".to_string(),
            size: "rest".to_string(),
            filesystem: Some("ext4".to_string()),
            mount_point: Some("/".to_string()),
            flags: None,
            volume_group: None,
        }
    }

    // ========== inject_boot_partition: no boot_mode ==========

    #[test]
    fn test_inject_none_boot_mode_is_noop() {
        let mut layout = root_labeled_disk_gpt(vec![root_partition()]);
        inject_boot_partition(&mut layout, None);
        assert_eq!(layout.disks[0].partitions.len(), 1);
    }

    // ========== inject_boot_partition: path-based layout (no ROOT label) ==========

    #[test]
    fn test_inject_path_based_layout_is_noop_for_uefi() {
        // When device = "/dev/sda", there is no ROOT label — no injection should occur
        // even with UEFI mode, because operator-defined path-based layouts are explicit.
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        inject_boot_partition(&mut layout, Some(FirmwareMode::Uefi));
        assert_eq!(
            layout.disks[0].partitions.len(),
            1,
            "path-based layout must not receive injected boot partition"
        );
    }

    #[test]
    fn test_inject_path_based_layout_is_noop_for_bios() {
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/sda".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        inject_boot_partition(&mut layout, Some(FirmwareMode::Bios));
        assert_eq!(
            layout.disks[0].partitions.len(),
            1,
            "path-based layout must not receive injected boot partition"
        );
    }

    // ========== inject_boot_partition: layout with DATA1 but no ROOT ==========

    #[test]
    fn test_inject_no_root_disk_is_noop() {
        // A layout with labelled disks but no ROOT label should not inject into any disk.
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "DATA1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        inject_boot_partition(&mut layout, Some(FirmwareMode::Uefi));
        assert_eq!(
            layout.disks[0].partitions.len(),
            1,
            "without ROOT disk no injection must occur"
        );
    }

    // ========== inject_boot_partition: UEFI ==========

    #[test]
    fn test_inject_uefi_adds_esp_partition_at_front() {
        let mut layout = root_labeled_disk_gpt(vec![root_partition()]);
        inject_boot_partition(&mut layout, Some(FirmwareMode::Uefi));
        let partitions = &layout.disks[0].partitions;
        assert_eq!(
            partitions.len(),
            2,
            "expected 2 partitions after ESP injection"
        );
        let esp = &partitions[0];
        assert_eq!(esp.label, "efi");
        assert_eq!(esp.size, "300MiB");
        assert_eq!(esp.filesystem, Some("vfat".to_string()));
        assert_eq!(esp.mount_point, Some("/boot/efi".to_string()));
        assert!(
            esp.flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"esp".to_string()),
            "injected partition must carry the esp flag"
        );
        // Root partition should remain last.
        assert_eq!(partitions[1].label, "root");
    }

    #[test]
    fn test_inject_uefi_skips_if_esp_already_present() {
        let mut layout = root_labeled_disk_gpt(vec![
            PartitionConfig {
                label: "existing_efi".to_string(),
                size: "512MiB".to_string(),
                filesystem: Some("vfat".to_string()),
                mount_point: Some("/boot/efi".to_string()),
                flags: Some(vec!["esp".to_string()]),
                volume_group: None,
            },
            root_partition(),
        ]);
        inject_boot_partition(&mut layout, Some(FirmwareMode::Uefi));
        assert_eq!(
            layout.disks[0].partitions.len(),
            2,
            "should not inject when esp flag already present"
        );
        assert_eq!(layout.disks[0].partitions[0].label, "existing_efi");
    }

    // ========== inject_boot_partition: BIOS ==========

    #[test]
    fn test_inject_bios_gpt_adds_bios_grub_at_front() {
        let mut layout = root_labeled_disk_gpt(vec![root_partition()]);
        inject_boot_partition(&mut layout, Some(FirmwareMode::Bios));
        let partitions = &layout.disks[0].partitions;
        assert_eq!(
            partitions.len(),
            2,
            "expected 2 partitions after bios_grub injection"
        );
        let grub = &partitions[0];
        assert_eq!(grub.label, "bios_grub");
        assert_eq!(grub.size, "1MiB");
        assert!(
            grub.filesystem.is_none(),
            "bios_grub partition must have no filesystem"
        );
        assert!(
            grub.flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"bios_grub".to_string()),
            "injected partition must carry the bios_grub flag"
        );
        assert_eq!(partitions[1].label, "root");
    }

    #[test]
    fn test_inject_bios_msdos_skips_injection() {
        // bios_grub is only needed for GPT tables; msdos does not require it.
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "msdos".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        inject_boot_partition(&mut layout, Some(FirmwareMode::Bios));
        assert_eq!(
            layout.disks[0].partitions.len(),
            1,
            "bios_grub is not required for msdos partition tables"
        );
    }

    #[test]
    fn test_inject_bios_skips_if_bios_grub_already_present() {
        let mut layout = root_labeled_disk_gpt(vec![
            PartitionConfig {
                label: "biosboot".to_string(),
                size: "2MiB".to_string(),
                filesystem: None,
                mount_point: None,
                flags: Some(vec!["bios_grub".to_string()]),
                volume_group: None,
            },
            root_partition(),
        ]);
        inject_boot_partition(&mut layout, Some(FirmwareMode::Bios));
        assert_eq!(
            layout.disks[0].partitions.len(),
            2,
            "should not inject when bios_grub flag already present"
        );
        assert_eq!(layout.disks[0].partitions[0].label, "biosboot");
    }

    // ========== inject_boot_partition: multiple disks ==========

    #[test]
    fn test_inject_uefi_only_injects_on_root_not_data_disks() {
        // Only the ROOT-labelled disk should receive the injected ESP.
        // DATA1 disks must remain untouched.
        let mut layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "ROOT".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![root_partition()],
                },
                DiskConfig {
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
                },
            ],
            volume_groups: None,
            zfs_pools: None,
        };
        inject_boot_partition(&mut layout, Some(FirmwareMode::Uefi));
        assert_eq!(
            layout.disks[0].partitions.len(),
            2,
            "ROOT disk should have 2 partitions after ESP injection"
        );
        assert_eq!(layout.disks[0].partitions[0].label, "efi");
        assert_eq!(
            layout.disks[1].partitions.len(),
            1,
            "DATA1 disk must not receive an injected boot partition"
        );
        assert_eq!(layout.disks[1].partitions[0].label, "data");
    }
}
