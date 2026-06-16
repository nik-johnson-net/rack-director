//! Migration 20: Backfill boot partitions into roles.disk_layout.
//!
//! Before this migration, rack-director auto-injected a firmware-appropriate boot
//! partition (ESP for UEFI, bios_grub for BIOS+GPT) into the ROOT disk at request
//! time. Layouts stored in the database never contained these partitions.
//!
//! This migration makes layouts explicit by prepending the required boot partition(s)
//! to the ROOT disk of every role whose stored layout lacks them.
//!
//! Rules (mirror validate_boot_partitions in disk_layout/validate.rs):
//! - firmware_mode = "uefi"  → prepend ESP (300 MiB vfat /boot/efi, flags=[esp]) if absent
//! - firmware_mode = "bios"  → prepend bios_grub (1 MiB, flags=[bios_grub]) if GPT and absent
//! - firmware_mode = NULL    → prepend ESP if absent; also prepend bios_grub if GPT and absent
//! - Path-based layouts (no ROOT label disk) → skipped

use crate::database::Connection;
use anyhow::Result;
use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig};

/// Prepend the required boot partition(s) to the ROOT disk of every role whose
/// stored layout lacks them, based on `firmware_mode`.
///
/// Roles using path-based layouts (no disk with `device == "ROOT"`) are skipped.
pub async fn backfill_boot_partitions(conn: &Connection) -> Result<()> {
    log::info!("Backfilling boot partitions into role disk layouts...");

    let rows: Vec<(i64, String, Option<String>)> = conn
        .query(
            "SELECT id, disk_layout, firmware_mode FROM roles",
            (),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .await?;

    let mut updated_count = 0;

    for (id, layout_json, firmware_mode) in rows {
        let mut layout: DiskLayout = serde_json::from_str(&layout_json)?;

        if backfill_layout(&mut layout, firmware_mode.as_deref()) {
            let new_json = serde_json::to_string(&layout)?;
            conn.execute(
                "UPDATE roles SET disk_layout = ?1 WHERE id = ?2",
                (new_json, id),
            )
            .await?;
            log::debug!("Backfilled boot partition(s) for role {}", id);
            updated_count += 1;
        }
    }

    log::info!("Updated {} role(s)", updated_count);
    Ok(())
}

/// Mutate `layout` to prepend required boot partition(s) based on `firmware_mode`.
///
/// Returns `true` if the layout was modified, `false` if it was already correct or
/// if the layout uses path-based device references instead of platform labels.
fn backfill_layout(layout: &mut DiskLayout, firmware_mode: Option<&str>) -> bool {
    let Some(root_disk) = layout.disks.iter_mut().find(|d| d.device == "ROOT") else {
        return false; // path-based layout — skip
    };

    match firmware_mode {
        Some("uefi") => backfill_esp(root_disk),
        Some("bios") => backfill_bios_grub(root_disk),
        None => {
            // Unspecified — add both ESP and bios_grub (for GPT) if absent.
            // bios_grub is inserted first (position 0), then ESP is inserted at
            // position 0, yielding final order: [esp, bios_grub, ...].
            let added_bios = backfill_bios_grub(root_disk);
            let added_esp = backfill_esp(root_disk);
            added_esp || added_bios
        }
        Some(_) => false, // unknown firmware_mode string — leave unchanged
    }
}

/// Prepend an EFI System Partition to `disk` if the `esp` flag is not already present.
///
/// Returns `true` if the partition was added.
fn backfill_esp(disk: &mut DiskConfig) -> bool {
    if has_flag(&disk.partitions, "esp") {
        return false;
    }
    disk.partitions.insert(
        0,
        PartitionConfig {
            label: "efi".to_string(),
            size: "300MiB".to_string(),
            filesystem: Some("vfat".to_string()),
            mount_point: Some("/boot/efi".to_string()),
            flags: Some(vec!["esp".to_string()]),
            volume_group: None,
        },
    );
    true
}

/// Prepend a BIOS GRUB partition to `disk` if the partition table is GPT and
/// the `bios_grub` flag is not already present.
///
/// Returns `true` if the partition was added.
fn backfill_bios_grub(disk: &mut DiskConfig) -> bool {
    if disk.partition_table != "gpt" || has_flag(&disk.partitions, "bios_grub") {
        return false;
    }
    disk.partitions.insert(
        0,
        PartitionConfig {
            label: "bios_grub".to_string(),
            size: "1MiB".to_string(),
            filesystem: None,
            mount_point: None,
            flags: Some(vec!["bios_grub".to_string()]),
            volume_group: None,
        },
    );
    true
}

/// Returns `true` if any partition in `partitions` carries the given `flag`.
fn has_flag(partitions: &[PartitionConfig], flag: &str) -> bool {
    partitions.iter().any(|p| {
        p.flags
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|f| f == flag)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig};

    fn make_gpt_layout(partitions: Vec<PartitionConfig>) -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions,
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        }
    }

    fn make_msdos_layout(partitions: Vec<PartitionConfig>) -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "msdos".to_string(),
                partitions,
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
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

    fn esp_partition() -> PartitionConfig {
        PartitionConfig {
            label: "efi".to_string(),
            size: "300MiB".to_string(),
            filesystem: Some("vfat".to_string()),
            mount_point: Some("/boot/efi".to_string()),
            flags: Some(vec!["esp".to_string()]),
            volume_group: None,
        }
    }

    fn bios_grub_partition() -> PartitionConfig {
        PartitionConfig {
            label: "bios_grub".to_string(),
            size: "1MiB".to_string(),
            filesystem: None,
            mount_point: None,
            flags: Some(vec!["bios_grub".to_string()]),
            volume_group: None,
        }
    }

    #[test]
    fn test_backfill_uefi_adds_esp() {
        let mut layout = make_gpt_layout(vec![root_partition()]);
        let changed = backfill_layout(&mut layout, Some("uefi"));

        assert!(changed, "Should report that a change was made");
        let partitions = &layout.disks[0].partitions;
        assert_eq!(partitions.len(), 2, "ESP should be prepended");
        assert_eq!(partitions[0].label, "efi");
        assert_eq!(partitions[0].size, "300MiB");
        assert_eq!(partitions[0].filesystem.as_deref(), Some("vfat"));
        assert_eq!(partitions[0].mount_point.as_deref(), Some("/boot/efi"));
        assert!(
            partitions[0]
                .flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"esp".to_string()),
            "ESP partition must carry the esp flag"
        );
        assert_eq!(partitions[1].label, "root", "Existing partition preserved");
    }

    #[test]
    fn test_backfill_uefi_skips_if_esp_present() {
        let mut layout = make_gpt_layout(vec![esp_partition(), root_partition()]);
        let changed = backfill_layout(&mut layout, Some("uefi"));

        assert!(
            !changed,
            "Should not modify layout when ESP is already present"
        );
        assert_eq!(
            layout.disks[0].partitions.len(),
            2,
            "Partition count should be unchanged"
        );
    }

    #[test]
    fn test_backfill_bios_gpt_adds_bios_grub() {
        let mut layout = make_gpt_layout(vec![root_partition()]);
        let changed = backfill_layout(&mut layout, Some("bios"));

        assert!(changed, "Should report that a change was made");
        let partitions = &layout.disks[0].partitions;
        assert_eq!(partitions.len(), 2, "bios_grub should be prepended");
        assert_eq!(partitions[0].label, "bios_grub");
        assert_eq!(partitions[0].size, "1MiB");
        assert!(partitions[0].filesystem.is_none());
        assert!(
            partitions[0]
                .flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"bios_grub".to_string()),
            "BIOS GRUB partition must carry the bios_grub flag"
        );
        assert_eq!(partitions[1].label, "root", "Existing partition preserved");
    }

    #[test]
    fn test_backfill_bios_msdos_skips() {
        // bios_grub is only needed for GPT; msdos tables don't need it
        let mut layout = make_msdos_layout(vec![root_partition()]);
        let changed = backfill_layout(&mut layout, Some("bios"));

        assert!(
            !changed,
            "Should not add bios_grub to an msdos partition table"
        );
        assert_eq!(layout.disks[0].partitions.len(), 1);
    }

    #[test]
    fn test_backfill_none_adds_both_on_gpt() {
        // firmware_mode=None means support both UEFI and BIOS; prepend both partitions.
        // Expected final order: [esp, bios_grub, root]
        let mut layout = make_gpt_layout(vec![root_partition()]);
        let changed = backfill_layout(&mut layout, None);

        assert!(changed, "Should report that changes were made");
        let partitions = &layout.disks[0].partitions;
        assert_eq!(
            partitions.len(),
            3,
            "Both ESP and bios_grub should be prepended"
        );
        assert_eq!(partitions[0].label, "efi", "ESP must be first (position 0)");
        assert_eq!(
            partitions[1].label, "bios_grub",
            "bios_grub must be second (position 1)"
        );
        assert_eq!(partitions[2].label, "root", "root partition must be last");
    }

    #[test]
    fn test_backfill_path_based_skips() {
        // Layouts with path-based device references have no ROOT disk and should be skipped.
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let original_partitions = layout.disks[0].partitions.clone();
        let changed = backfill_layout(&mut layout, Some("uefi"));

        assert!(!changed, "Path-based layouts should be skipped");
        assert_eq!(
            layout.disks[0].partitions, original_partitions,
            "Partitions should be unchanged"
        );
    }

    #[test]
    fn test_backfill_idempotent() {
        // A layout that already has both ESP and bios_grub should be untouched.
        let mut layout = make_gpt_layout(vec![
            esp_partition(),
            bios_grub_partition(),
            root_partition(),
        ]);
        let changed = backfill_layout(&mut layout, None);

        assert!(
            !changed,
            "Layout already containing both partitions should not be modified"
        );
        assert_eq!(
            layout.disks[0].partitions.len(),
            3,
            "Partition count should be unchanged"
        );
        assert_eq!(layout.disks[0].partitions[0].label, "efi");
        assert_eq!(layout.disks[0].partitions[1].label, "bios_grub");
        assert_eq!(layout.disks[0].partitions[2].label, "root");
    }

    #[test]
    fn test_backfill_none_msdos_adds_only_esp() {
        // firmware_mode=None on msdos: GRUB embeds in MBR gap, only ESP should be added.
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "msdos".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let changed = backfill_layout(&mut layout, None);
        assert!(changed, "ESP should be added");
        let partitions = &layout.disks[0].partitions;
        assert_eq!(partitions.len(), 2, "should have ESP + root");
        assert!(
            partitions[0]
                .flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"esp".to_string()),
            "first partition must be ESP"
        );
        assert!(
            !partitions.iter().any(|p| p
                .flags
                .as_deref()
                .unwrap_or_default()
                .contains(&"bios_grub".to_string())),
            "bios_grub must NOT be added on msdos"
        );
    }

    #[test]
    fn test_backfill_unknown_firmware_mode_skips() {
        let mut layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![root_partition()],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let changed = backfill_layout(&mut layout, Some("riscv"));
        assert!(!changed, "unknown firmware_mode must not modify the layout");
        assert_eq!(layout.disks[0].partitions.len(), 1);
    }
}
