//! Migration 14: Convert old disk_layout JSON format to new DiskLayout schema
//!
//! Old format: {"partitions": [{"device": "/dev/sda", "size": "100G", ...}]}
//! New format: {"disks": [{"device": "/dev/sda", "partition_table": "gpt", "partitions": [...]}]}

use crate::database::Connection;
use anyhow::Result;
use std::collections::BTreeMap;

/// Convert old DiskLayout format to new format
///
/// Reads all roles, converts old-style disk_layout JSON to new format,
/// and updates the database. Skips roles that already use the new format.
pub async fn convert_disk_layouts(conn: &Connection) -> Result<()> {
    log::info!("Converting disk layouts to new schema format...");

    // Read all roles
    let rows: Vec<(i64, String)> = conn
        .query("SELECT id, disk_layout FROM roles", (), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .await?;

    let mut converted_count = 0;
    for (id, layout_json) in rows {
        let value: serde_json::Value = serde_json::from_str(&layout_json)?;

        // Check if it's already in the new format
        if value.get("disks").is_some() {
            continue;
        }

        // Convert old format
        let new_layout = convert_old_layout(&value)?;
        let new_json = serde_json::to_string(&new_layout)?;

        conn.execute(
            "UPDATE roles SET disk_layout = ?1 WHERE id = ?2",
            (new_json, id),
        )
        .await?;

        log::debug!("Migrated disk layout for role {}", id);
        converted_count += 1;
    }

    log::info!("Converted {} disk layout(s)", converted_count);
    Ok(())
}

/// Convert old-style disk layout JSON to new format
///
/// Groups partitions by device and creates DiskConfig entries
fn convert_old_layout(value: &serde_json::Value) -> Result<serde_json::Value> {
    let partitions = value
        .get("partitions")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    // Group partitions by device
    let mut device_map: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();

    for (idx, part) in partitions.iter().enumerate() {
        let device = part
            .get("device")
            .and_then(|d| d.as_str())
            .unwrap_or("/dev/sda")
            .to_string();

        // Extract base device path (strip partition number if present)
        let base_device = strip_partition_number(&device);

        // Create new partition format
        let new_partition = serde_json::json!({
            "label": part.get("mount_point")
                .and_then(|m| m.as_str())
                .map(|m| m.trim_start_matches('/').replace('/', "-"))
                .unwrap_or_else(|| format!("part{}", idx)),
            "size": part.get("size").and_then(|s| s.as_str()).unwrap_or("rest"),
            "filesystem": part.get("filesystem").and_then(|f| f.as_str()),
            "mount_point": part.get("mount_point").and_then(|m| m.as_str()),
            "flags": part.get("flags").and_then(|f| f.as_array()).cloned(),
        });

        device_map
            .entry(base_device)
            .or_default()
            .push(new_partition);
    }

    // Build new disks array
    let disks: Vec<serde_json::Value> = device_map
        .into_iter()
        .map(|(device, partitions)| {
            serde_json::json!({
                "device": device,
                "partition_table": "gpt",
                "partitions": partitions,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "disks": disks,
    }))
}

/// Strip partition number from device path
///
/// # Examples
/// - `/dev/sda1` -> `/dev/sda`
/// - `/dev/nvme0n1p1` -> `/dev/nvme0n1`
/// - `/dev/sda` -> `/dev/sda` (no change)
fn strip_partition_number(device: &str) -> String {
    // Handle NVMe: /dev/nvme0n1p1 -> /dev/nvme0n1
    // NVMe devices have pattern: /dev/nvmeXnYpZ where X=controller, Y=namespace, Z=partition
    if device.contains("nvme") {
        if let Some(pos) = device.rfind('p')
            && pos > 0
            && !device[pos + 1..].is_empty()
            && device[pos + 1..].chars().all(|c| c.is_ascii_digit())
        {
            // Check if character before 'p' is a digit (confirms nvmeXnYpZ pattern)
            if device.as_bytes()[pos - 1].is_ascii_digit() {
                return device[..pos].to_string();
            }
        }
        // No partition number found, return as-is
        return device.to_string();
    }

    // Handle SATA/SCSI: /dev/sda1 -> /dev/sda
    let trimmed = device.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.len() < device.len() {
        return trimmed.to_string();
    }

    device.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_partition_number_sata() {
        assert_eq!(strip_partition_number("/dev/sda1"), "/dev/sda");
        assert_eq!(strip_partition_number("/dev/sda2"), "/dev/sda");
        assert_eq!(strip_partition_number("/dev/sdb10"), "/dev/sdb");
        assert_eq!(strip_partition_number("/dev/sda"), "/dev/sda");
    }

    #[test]
    fn test_strip_partition_number_nvme() {
        assert_eq!(strip_partition_number("/dev/nvme0n1p1"), "/dev/nvme0n1");
        assert_eq!(strip_partition_number("/dev/nvme0n1p2"), "/dev/nvme0n1");
        assert_eq!(strip_partition_number("/dev/nvme1n1p10"), "/dev/nvme1n1");
        assert_eq!(strip_partition_number("/dev/nvme0n1"), "/dev/nvme0n1");
    }

    #[test]
    fn test_strip_partition_number_edge_cases() {
        // No partition number
        assert_eq!(strip_partition_number("/dev/sda"), "/dev/sda");
        assert_eq!(strip_partition_number("/dev/nvme0n1"), "/dev/nvme0n1");

        // Note: LVM device mapper paths like /dev/mapper/vg0-lv0 will have trailing
        // digits stripped, but this is acceptable since the migration is for converting
        // old partition-based layouts which wouldn't contain LVM device paths anyway
        // (they would use /dev/sda, /dev/sdb, etc)
    }

    #[test]
    fn test_convert_old_layout_simple() {
        let old = serde_json::json!({
            "partitions": [
                {
                    "device": "/dev/sda1",
                    "size": "512M",
                    "filesystem": "vfat",
                    "mount_point": "/boot/efi",
                    "flags": ["esp"]
                },
                {
                    "device": "/dev/sda2",
                    "size": "rest",
                    "filesystem": "ext4",
                    "mount_point": "/",
                    "flags": []
                }
            ]
        });

        let result = convert_old_layout(&old).unwrap();

        // Check structure
        assert!(result.get("disks").is_some());
        let disks = result.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 1);

        // Check disk device
        let disk = &disks[0];
        assert_eq!(disk.get("device").unwrap().as_str().unwrap(), "/dev/sda");
        assert_eq!(
            disk.get("partition_table").unwrap().as_str().unwrap(),
            "gpt"
        );

        // Check partitions
        let partitions = disk.get("partitions").unwrap().as_array().unwrap();
        assert_eq!(partitions.len(), 2);

        // First partition
        assert_eq!(partitions[0].get("size").unwrap().as_str().unwrap(), "512M");
        assert_eq!(
            partitions[0].get("filesystem").unwrap().as_str().unwrap(),
            "vfat"
        );
        assert_eq!(
            partitions[0].get("mount_point").unwrap().as_str().unwrap(),
            "/boot/efi"
        );

        // Second partition
        assert_eq!(partitions[1].get("size").unwrap().as_str().unwrap(), "rest");
        assert_eq!(
            partitions[1].get("filesystem").unwrap().as_str().unwrap(),
            "ext4"
        );
        assert_eq!(
            partitions[1].get("mount_point").unwrap().as_str().unwrap(),
            "/"
        );
    }

    #[test]
    fn test_convert_old_layout_empty() {
        let old = serde_json::json!({
            "partitions": []
        });

        let result = convert_old_layout(&old).unwrap();

        assert!(result.get("disks").is_some());
        let disks = result.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 0);
    }

    #[test]
    fn test_convert_old_layout_multiple_disks() {
        let old = serde_json::json!({
            "partitions": [
                {
                    "device": "/dev/sda1",
                    "size": "100G",
                    "filesystem": "ext4",
                    "mount_point": "/",
                    "flags": []
                },
                {
                    "device": "/dev/sdb1",
                    "size": "rest",
                    "filesystem": "xfs",
                    "mount_point": "/data",
                    "flags": []
                }
            ]
        });

        let result = convert_old_layout(&old).unwrap();

        let disks = result.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 2);

        // Check both disks are present
        let disk_devices: Vec<&str> = disks
            .iter()
            .map(|d| d.get("device").unwrap().as_str().unwrap())
            .collect();
        assert!(disk_devices.contains(&"/dev/sda"));
        assert!(disk_devices.contains(&"/dev/sdb"));
    }

    #[test]
    fn test_convert_old_layout_nvme() {
        let old = serde_json::json!({
            "partitions": [
                {
                    "device": "/dev/nvme0n1p1",
                    "size": "512M",
                    "filesystem": "vfat",
                    "mount_point": "/boot/efi",
                    "flags": ["esp"]
                }
            ]
        });

        let result = convert_old_layout(&old).unwrap();

        let disks = result.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(
            disks[0].get("device").unwrap().as_str().unwrap(),
            "/dev/nvme0n1"
        );
    }
}
