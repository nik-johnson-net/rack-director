//! Migration 15: Strip `path` from each disk entry in `platforms.attributes` JSON.
//!
//! `PlatformDisk` previously stored a `path` field containing the by-path device string
//! observed on whichever device first created the platform. This was brittle: two
//! identical servers with different PCIe bus topologies would resolve labels to the wrong
//! disks. The field is removed so that platforms describe hardware class only.

use crate::database::Connection;
use anyhow::Result;

/// Remove the `path` field from every disk entry in all platform `attributes` JSON blobs.
///
/// Iterates over all rows in the `platforms` table, parses the `attributes` JSON,
/// removes `path` from each disk in `attributes.disks`, and writes the cleaned JSON
/// back. Rows whose disk entries already lack `path` are skipped unchanged.
pub async fn strip_disk_paths(conn: &Connection) -> Result<()> {
    log::info!("Stripping path field from PlatformDisk entries...");

    let rows: Vec<(i64, String)> = conn
        .query("SELECT id, attributes FROM platforms", (), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .await?;

    let mut updated_count = 0;
    for (id, attrs_json) in rows {
        let mut attrs: serde_json::Value = serde_json::from_str(&attrs_json)?;

        if !remove_disk_paths(&mut attrs) {
            // No path fields present; nothing to do for this row
            continue;
        }

        let new_json = serde_json::to_string(&attrs)?;
        conn.execute(
            "UPDATE platforms SET attributes = ?1 WHERE id = ?2",
            (new_json, id),
        )
        .await?;

        log::debug!("Removed path from platform {} disk entries", id);
        updated_count += 1;
    }

    log::info!("Updated {} platform(s)", updated_count);
    Ok(())
}

/// Remove the `path` field from every entry in `attrs["disks"]`.
///
/// Returns `true` if any `path` field was found and removed, `false` if the JSON was
/// already clean. This allows callers to skip the database write when nothing changed.
fn remove_disk_paths(attrs: &mut serde_json::Value) -> bool {
    let disks = match attrs.get_mut("disks").and_then(|d| d.as_array_mut()) {
        Some(d) => d,
        None => return false,
    };

    let mut any_removed = false;
    for disk in disks.iter_mut() {
        if let Some(obj) = disk.as_object_mut()
            && obj.remove("path").is_some()
        {
            any_removed = true;
        }
    }

    any_removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_remove_disk_paths_removes_path() {
        let mut attrs = json!({
            "disks": [
                { "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1", "size_gb": 480, "disk_type": "ssd", "label": "ROOT" },
                { "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-2", "size_gb": 2000, "disk_type": "hdd", "label": "DATA1" }
            ],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        });

        let changed = remove_disk_paths(&mut attrs);

        assert!(changed, "Should report that changes were made");

        let disks = attrs["disks"].as_array().unwrap();
        assert!(
            disks[0].get("path").is_none(),
            "path should be removed from first disk"
        );
        assert!(
            disks[1].get("path").is_none(),
            "path should be removed from second disk"
        );

        // Other fields must be preserved
        assert_eq!(disks[0]["size_gb"], json!(480));
        assert_eq!(disks[0]["disk_type"], json!("ssd"));
        assert_eq!(disks[0]["label"], json!("ROOT"));
        assert_eq!(disks[1]["size_gb"], json!(2000));
    }

    #[test]
    fn test_remove_disk_paths_no_path_unchanged() {
        let mut attrs = json!({
            "disks": [
                { "size_gb": 480, "disk_type": "ssd", "label": "ROOT" }
            ],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        });

        let changed = remove_disk_paths(&mut attrs);

        assert!(
            !changed,
            "Should report no changes when path is already absent"
        );
    }

    #[test]
    fn test_remove_disk_paths_no_disks_key() {
        let mut attrs = json!({
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        });

        let changed = remove_disk_paths(&mut attrs);

        assert!(
            !changed,
            "Should report no changes when disks key is absent"
        );
    }

    #[test]
    fn test_remove_disk_paths_empty_disks() {
        let mut attrs = json!({
            "disks": [],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        });

        let changed = remove_disk_paths(&mut attrs);

        assert!(
            !changed,
            "Should report no changes for an empty disks array"
        );
    }

    #[test]
    fn test_remove_disk_paths_mixed_presence() {
        // One disk has path, another does not (e.g. partially migrated data)
        let mut attrs = json!({
            "disks": [
                { "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1", "size_gb": 480, "disk_type": "ssd", "label": "ROOT" },
                { "size_gb": 2000, "disk_type": "hdd", "label": "DATA1" }
            ],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        });

        let changed = remove_disk_paths(&mut attrs);

        assert!(changed, "Should report changes when any path was removed");
        let disks = attrs["disks"].as_array().unwrap();
        assert!(disks[0].get("path").is_none());
        assert!(disks[1].get("path").is_none());
    }
}
