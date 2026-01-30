use super::{DiskLayout, Role, RoleWithOs};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use rusqlite::{Connection, params};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct RolesStore {
    db: Arc<Mutex<Connection>>,
}

impl RolesStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Create a new role
    pub async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        os_id: i64,
        disk_layout: &DiskLayout,
        config_template: Option<&serde_json::Value>,
    ) -> Result<Role> {
        let conn = self.db.lock().await;
        let now = Utc::now();
        let disk_layout_json = serde_json::to_string(disk_layout)?;
        let config_json = config_template.map(serde_json::to_string).transpose()?;

        conn.execute(
            "INSERT INTO roles (name, description, os_id, disk_layout, config_template, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![name, description, os_id, disk_layout_json, config_json, now, now],
        )
        .context("Failed to insert role")?;

        let id = conn.last_insert_rowid();

        Ok(Role {
            id: Some(id),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            os_id,
            disk_layout: disk_layout.clone(),
            config_template: config_template.cloned(),
            created_at: Some(now),
            updated_at: Some(now),
        })
    }

    /// Get a role by ID
    pub async fn get(&self, id: i64) -> Result<Role> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, name, description, os_id, disk_layout, config_template, created_at, updated_at
             FROM roles WHERE id = ?1",
        )?;

        let role = stmt
            .query_row(params![id], |row| {
                let disk_layout_json: String = row.get(4)?;
                let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
                let config_json: Option<String> = row.get(5)?;
                let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Role {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    os_id: row.get(3)?,
                    disk_layout,
                    config_template,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .context("Role not found")?;

        Ok(role)
    }

    /// Get a role with its associated OS information
    pub async fn get_with_os(&self, id: i64) -> Result<RoleWithOs> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare(
            "SELECT r.id, r.name, r.description, r.os_id, r.disk_layout, r.config_template,
                    r.created_at, r.updated_at, o.name, o.version
             FROM roles r
             JOIN operating_systems o ON r.os_id = o.id
             WHERE r.id = ?1",
        )?;

        let role = stmt
            .query_row(params![id], |row| {
                let disk_layout_json: String = row.get(4)?;
                let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
                let config_json: Option<String> = row.get(5)?;
                let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

                Ok(RoleWithOs {
                    role: Role {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        os_id: row.get(3)?,
                        disk_layout,
                        config_template,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    },
                    os_name: row.get(8)?,
                    os_version: row.get(9)?,
                })
            })
            .context("Role not found")?;

        Ok(role)
    }

    /// List all roles (only used in tests)
    #[cfg(test)]
    pub async fn list(&self) -> Result<Vec<Role>> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, name, description, os_id, disk_layout, config_template, created_at, updated_at
             FROM roles ORDER BY name",
        )?;

        let rows = stmt.query_map(params![], |row| {
            let disk_layout_json: String = row.get(4)?;
            let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
            let config_json: Option<String> = row.get(5)?;
            let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Role {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                os_id: row.get(3)?,
                disk_layout,
                config_template,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;

        let mut roles = Vec::new();
        for row in rows {
            roles.push(row?);
        }

        Ok(roles)
    }

    /// List all roles with their OS information
    pub async fn list_with_os(&self) -> Result<Vec<RoleWithOs>> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare(
            "SELECT r.id, r.name, r.description, r.os_id, r.disk_layout, r.config_template,
                    r.created_at, r.updated_at, o.name, o.version
             FROM roles r
             JOIN operating_systems o ON r.os_id = o.id
             ORDER BY r.name",
        )?;

        let rows = stmt.query_map(params![], |row| {
            let disk_layout_json: String = row.get(4)?;
            let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
            let config_json: Option<String> = row.get(5)?;
            let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

            Ok(RoleWithOs {
                role: Role {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    os_id: row.get(3)?,
                    disk_layout,
                    config_template,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                },
                os_name: row.get(8)?,
                os_version: row.get(9)?,
            })
        })?;

        let mut roles = Vec::new();
        for row in rows {
            roles.push(row?);
        }

        Ok(roles)
    }

    /// Update a role
    pub async fn update(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        os_id: Option<i64>,
        disk_layout: Option<&DiskLayout>,
        config_template: Option<&serde_json::Value>,
    ) -> Result<Role> {
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
            if let Some(os_id) = os_id {
                updates.push("os_id = ?");
                params.push(Box::new(os_id));
            }
            if let Some(disk_layout) = disk_layout {
                updates.push("disk_layout = ?");
                let json = serde_json::to_string(disk_layout)?;
                params.push(Box::new(json));
            }
            if let Some(config_template) = config_template {
                updates.push("config_template = ?");
                let json = serde_json::to_string(config_template)?;
                params.push(Box::new(json));
            }

            if updates.is_empty() {
                false
            } else {
                updates.push("updated_at = ?");
                params.push(Box::new(now));
                params.push(Box::new(id));

                let query = format!("UPDATE roles SET {} WHERE id = ?", updates.join(", "));

                conn.execute(&query, rusqlite::params_from_iter(params.iter()))?;
                true
            }
        };

        if !needs_update {
            return self.get(id).await;
        }

        self.get(id).await
    }

    /// Delete a role
    pub async fn delete(&self, id: i64) -> Result<()> {
        let conn = self.db.lock().await;

        let rows_affected = conn
            .execute("DELETE FROM roles WHERE id = ?1", params![id])
            .context("Failed to delete role")?;

        if rows_affected == 0 {
            return Err(anyhow!("Role not found"));
        }

        Ok(())
    }

    /// Assign a role to a device
    pub async fn assign_to_device(&self, device_uuid: &Uuid, role_id: i64) -> Result<()> {
        let conn = self.db.lock().await;

        conn.execute(
            "UPDATE devices SET role_id = ?1 WHERE uuid = ?2",
            params![role_id, device_uuid.to_string()],
        )
        .context("Failed to assign role to device")?;

        Ok(())
    }

    /// Get the role assigned to a device
    pub async fn get_device_role(&self, device_uuid: &Uuid) -> Result<Option<Role>> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare(
            "SELECT r.id, r.name, r.description, r.os_id, r.disk_layout, r.config_template, r.created_at, r.updated_at
             FROM roles r
             JOIN devices d ON d.role_id = r.id
             WHERE d.uuid = ?1",
        )?;

        let result = stmt.query_row(params![device_uuid.to_string()], |row| {
            let disk_layout_json: String = row.get(4)?;
            let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
            let config_json: Option<String> = row.get(5)?;
            let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Role {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                os_id: row.get(3)?,
                disk_layout,
                config_template,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        });

        match result {
            Ok(role) => Ok(Some(role)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all devices with a specific role
    pub async fn list_devices_with_role(&self, role_id: i64) -> Result<Vec<Uuid>> {
        let conn = self.db.lock().await;

        let mut stmt = conn.prepare("SELECT uuid FROM devices WHERE role_id = ?1 ORDER BY uuid")?;

        let rows = stmt.query_map(params![role_id], |row| row.get(0))?;

        let mut uuids = Vec::new();
        for row in rows {
            uuids.push(row?);
        }

        Ok(uuids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::operating_systems::OperatingSystemsStore;
    use crate::roles::{DiskLayout, Partition};

    fn setup_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        database::run_migrations(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[tokio::test]
    async fn test_create_and_get_role() {
        let db = setup_db();
        let os_store = OperatingSystemsStore::new(db.clone());
        let role_store = RolesStore::new(db);

        // Create OS first
        let os = os_store.create("Ubuntu", "24.04", None).await.unwrap();

        // Create role
        let disk_layout = DiskLayout {
            partitions: vec![Partition {
                device: "/dev/sda1".to_string(),
                size: "100G".to_string(),
                filesystem: "ext4".to_string(),
                mount_point: Some("/".to_string()),
                flags: vec![],
            }],
        };

        let role = role_store
            .create(
                "web-server",
                Some("Web server role"),
                os.id.unwrap(),
                &disk_layout,
                None,
            )
            .await
            .unwrap();

        assert!(role.id.is_some());
        assert_eq!(role.name, "web-server");

        let retrieved = role_store.get(role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, role.name);
        assert_eq!(retrieved.disk_layout.partitions.len(), 1);
    }

    #[tokio::test]
    async fn test_list_roles() {
        let db = setup_db();
        let os_store = OperatingSystemsStore::new(db.clone());
        let role_store = RolesStore::new(db);

        let os = os_store.create("Ubuntu", "24.04", None).await.unwrap();
        let disk_layout = DiskLayout { partitions: vec![] };

        role_store
            .create("role1", None, os.id.unwrap(), &disk_layout, None)
            .await
            .unwrap();
        role_store
            .create("role2", None, os.id.unwrap(), &disk_layout, None)
            .await
            .unwrap();

        let list = role_store.list().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_get_with_os() {
        let db = setup_db();
        let os_store = OperatingSystemsStore::new(db.clone());
        let role_store = RolesStore::new(db);

        let os = os_store.create("Ubuntu", "24.04", None).await.unwrap();
        let disk_layout = DiskLayout { partitions: vec![] };

        let role = role_store
            .create("web-server", None, os.id.unwrap(), &disk_layout, None)
            .await
            .unwrap();

        let role_with_os = role_store.get_with_os(role.id.unwrap()).await.unwrap();
        assert_eq!(role_with_os.os_name, "Ubuntu");
        assert_eq!(role_with_os.os_version, "24.04");
    }

    #[tokio::test]
    async fn test_update_role() {
        let db = setup_db();
        let os_store = OperatingSystemsStore::new(db.clone());
        let role_store = RolesStore::new(db);

        let os = os_store.create("Ubuntu", "24.04", None).await.unwrap();
        let disk_layout = DiskLayout { partitions: vec![] };

        let role = role_store
            .create("web-server", None, os.id.unwrap(), &disk_layout, None)
            .await
            .unwrap();

        let updated = role_store
            .update(
                role.id.unwrap(),
                Some("updated-name"),
                Some("New description"),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "updated-name");
        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_role() {
        let db = setup_db();
        let os_store = OperatingSystemsStore::new(db.clone());
        let role_store = RolesStore::new(db);

        let os = os_store.create("Ubuntu", "24.04", None).await.unwrap();
        let disk_layout = DiskLayout { partitions: vec![] };

        let role = role_store
            .create("web-server", None, os.id.unwrap(), &disk_layout, None)
            .await
            .unwrap();

        role_store.delete(role.id.unwrap()).await.unwrap();
        assert!(role_store.get(role.id.unwrap()).await.is_err());
    }
}
