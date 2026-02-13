use super::{Platform, PlatformAttributes};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use rusqlite::{Connection, params};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct PlatformsStore {
    db: Arc<Mutex<Connection>>,
}

impl PlatformsStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Create a new platform
    pub async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        attributes: &PlatformAttributes,
    ) -> Result<Platform> {
        let conn = self.db.lock().await;
        let now = Utc::now();
        let attributes_json = serde_json::to_string(attributes)?;

        conn.execute(
            "INSERT INTO platforms (name, description, attributes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, description, attributes_json, now, now],
        )
        .context("Failed to insert platform")?;

        let id = conn.last_insert_rowid();

        Ok(Platform {
            id: Some(id),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            attributes: attributes.clone(),
            created_at: Some(now),
            updated_at: Some(now),
        })
    }

    /// Get a platform by ID
    pub async fn get(&self, id: i64) -> Result<Platform> {
        let conn = self.db.lock().await;

        let platform = crate::database::query_one::<Platform>(
            &conn,
            "SELECT id, name, description, attributes, created_at, updated_at
             FROM platforms WHERE id = ?1",
            &[&id],
        )
        .context("Platform not found")?;

        Ok(platform)
    }

    /// List all platforms
    pub async fn list(&self) -> Result<Vec<Platform>> {
        let conn = self.db.lock().await;

        let platforms = crate::database::query_map_all::<Platform>(
            &conn,
            "SELECT id, name, description, attributes, created_at, updated_at
             FROM platforms ORDER BY name",
            &[],
        )?;

        Ok(platforms)
    }

    /// Update a platform
    pub async fn update(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        attributes: Option<&PlatformAttributes>,
    ) -> Result<Platform> {
        let needs_update = {
            let conn = self.db.lock().await;
            let now = Utc::now();

            let mut updates = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(name) = name {
                updates.push("name = ?");
                params.push(Box::new(name.to_string()));
            }
            if let Some(description) = description {
                updates.push("description = ?");
                params.push(Box::new(description.to_string()));
            }
            if let Some(attributes) = attributes {
                updates.push("attributes = ?");
                let json = serde_json::to_string(attributes)?;
                params.push(Box::new(json));
            }

            if updates.is_empty() {
                false
            } else {
                updates.push("updated_at = ?");
                params.push(Box::new(now));
                params.push(Box::new(id));

                let query = format!("UPDATE platforms SET {} WHERE id = ?", updates.join(", "));

                conn.execute(&query, rusqlite::params_from_iter(params.iter()))?;
                true
            }
        };

        if !needs_update {
            return self.get(id).await;
        }

        self.get(id).await
    }

    /// Delete a platform
    /// Returns an error if devices are assigned to this platform
    pub async fn delete(&self, id: i64) -> Result<()> {
        let conn = self.db.lock().await;

        // Use IMMEDIATE transaction to prevent race condition between check and delete
        conn.execute("BEGIN IMMEDIATE", [])
            .context("Failed to begin transaction")?;

        // Check if any devices are assigned to this platform
        let device_count: i64 = match conn.query_row(
            "SELECT COUNT(*) FROM devices WHERE platform_id = ?1",
            params![id],
            |row| row.get(0),
        ) {
            Ok(count) => count,
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(e.into());
            }
        };

        if device_count > 0 {
            let _ = conn.execute("ROLLBACK", []);
            return Err(anyhow!(
                "Cannot delete platform: {} device(s) are assigned to it",
                device_count
            ));
        }

        // Perform the delete
        let rows_affected = match conn.execute("DELETE FROM platforms WHERE id = ?1", params![id]) {
            Ok(rows) => rows,
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(anyhow::Error::from(e).context("Failed to delete platform"));
            }
        };

        if rows_affected == 0 {
            let _ = conn.execute("ROLLBACK", []);
            return Err(anyhow!("Platform not found"));
        }

        // Commit the transaction
        conn.execute("COMMIT", [])
            .context("Failed to commit transaction")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::operating_systems::Architecture;
    use crate::platforms::{DiskType, PlatformCpu, PlatformDisk, PlatformNic};
    use uuid::Uuid;

    fn setup_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        database::run_migrations(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn sample_platform_attributes() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:00:1f.2-ata-2".to_string(),
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![
                PlatformNic {
                    logical: "eno1".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC1".to_string()),
                },
                PlatformNic {
                    logical: "eno2".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC2".to_string()),
                },
            ],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_platform() {
        let db = setup_db();
        let store = PlatformsStore::new(db);

        let attrs = sample_platform_attributes();
        let platform = store
            .create("PowerEdge R640", Some("Dell server"), &attrs)
            .await
            .unwrap();

        assert!(platform.id.is_some());
        assert_eq!(platform.name, "PowerEdge R640");
        assert_eq!(platform.description, Some("Dell server".to_string()));
        assert_eq!(platform.attributes.disks.len(), 2);
        assert_eq!(platform.attributes.nics.len(), 2);
        assert_eq!(platform.attributes.cpus.len(), 1);
        assert_eq!(platform.attributes.memory_gib, 32);

        let retrieved = store.get(platform.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, platform.name);
        assert_eq!(retrieved.attributes.disks.len(), 2);
    }

    #[tokio::test]
    async fn test_list_platforms() {
        let db = setup_db();
        let store = PlatformsStore::new(db);

        let attrs = sample_platform_attributes();
        store.create("Platform 1", None, &attrs).await.unwrap();
        store.create("Platform 2", None, &attrs).await.unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_update_platform() {
        let db = setup_db();
        let store = PlatformsStore::new(db);

        let attrs = sample_platform_attributes();
        let platform = store.create("Original Name", None, &attrs).await.unwrap();

        let updated = store
            .update(
                platform.id.unwrap(),
                Some("Updated Name"),
                Some("New description"),
                None,
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_platform() {
        let db = setup_db();
        let store = PlatformsStore::new(db);

        let attrs = sample_platform_attributes();
        let platform = store.create("Test Platform", None, &attrs).await.unwrap();

        store.delete(platform.id.unwrap()).await.unwrap();
        assert!(store.get(platform.id.unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn test_delete_platform_with_devices_fails() {
        let db = setup_db();
        let store = PlatformsStore::new(db.clone());
        let director_store = crate::director::store::DirectorStore::new(db.clone());

        // Create platform
        let attrs = sample_platform_attributes();
        let platform = store.create("Test Platform", None, &attrs).await.unwrap();

        // Create device and assign platform
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440020").unwrap();
        {
            let conn = db.lock().await;
            conn.execute(
                "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
                params![device_uuid, Architecture::X86_64.as_str()],
            )
            .unwrap();
        }

        director_store
            .assign_platform_to_device(&device_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Try to delete platform - should fail
        let result = store.delete(platform.id.unwrap()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot delete"));
    }

    #[tokio::test]
    async fn test_concurrent_delete_and_assign_protection() {
        let db = setup_db();
        let store = PlatformsStore::new(db.clone());
        let director_store = crate::director::store::DirectorStore::new(db.clone());

        // Create platform
        let attrs = sample_platform_attributes();
        let platform = store.create("Test Platform", None, &attrs).await.unwrap();
        let platform_id = platform.id.unwrap();

        // Create device (without assigning platform yet)
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440021").unwrap();
        {
            let conn = db.lock().await;
            conn.execute(
                "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
                params![device_uuid, Architecture::X86_64.as_str()],
            )
            .unwrap();
        }

        // Spawn two concurrent tasks that race to complete:
        // Task A: Delete the platform
        // Task B: Assign the platform to the device
        let store_clone = store.clone();
        let director_store_clone = director_store.clone();

        let delete_task = tokio::spawn(async move {
            // Add small delay to increase chance of real race condition
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            store_clone.delete(platform_id).await
        });

        let assign_task = tokio::spawn(async move {
            // Add small delay to increase chance of real race condition
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            director_store_clone
                .assign_platform_to_device(&device_uuid, platform_id)
                .await
        });

        // Wait for both tasks to complete
        let (delete_result, assign_result) = tokio::join!(delete_task, assign_task);

        // Unwrap the JoinHandle results to get the actual operation results
        let delete_result = delete_result.expect("Delete task panicked");
        let assign_result = assign_result.expect("Assign task panicked");

        // CRITICAL: Only one should succeed
        // Either:
        // - Delete succeeds and assign fails (platform doesn't exist)
        // - Assign succeeds and delete fails (device is assigned)
        //
        // Both should NEVER succeed (that would be a data integrity violation)
        match (delete_result, assign_result) {
            (Ok(_), Err(_)) => {
                // Delete succeeded, assign failed - verify platform is gone
                let platform_check = store.get(platform_id).await;
                assert!(
                    platform_check.is_err(),
                    "Platform should not exist after successful delete"
                );
            }
            (Err(e), Ok(_)) => {
                // Assign succeeded, delete failed - verify error message
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("Cannot delete") || err_msg.contains("device(s) are assigned"),
                    "Delete should fail with correct error message, got: {}",
                    err_msg
                );

                // Verify device has the platform assigned
                let device_platform = director_store
                    .get_device_platform_id(&device_uuid)
                    .await
                    .unwrap();
                assert_eq!(
                    device_platform,
                    Some(platform_id),
                    "Device should have platform assigned"
                );
            }
            (Ok(_), Ok(_)) => {
                panic!(
                    "CRITICAL: Both delete and assign succeeded - transaction protection failed!"
                );
            }
            (Err(e1), Err(e2)) => {
                // Both failed - this shouldn't happen in normal operation but isn't a data integrity issue
                // One of them should succeed since they're operating on valid data
                panic!(
                    "Both operations failed unexpectedly:\nDelete error: {}\nAssign error: {}",
                    e1, e2
                );
            }
        }
    }
}
