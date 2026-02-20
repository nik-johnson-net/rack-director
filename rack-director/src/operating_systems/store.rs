use super::{Architecture, OperatingSystem, OsArchitecture};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;

use crate::database::Connection;

/// Complete operating system with all architecture configurations.
#[derive(Debug, Serialize)]
pub struct OperatingSystemWithArchitectures {
    #[serde(flatten)]
    pub os: OperatingSystem,
    pub architectures: Vec<OsArchitecture>,
}

#[derive(Clone)]
pub struct OperatingSystemsStore {
    db: Arc<Connection>,
}

impl OperatingSystemsStore {
    pub fn new(db: Arc<Connection>) -> Self {
        Self { db }
    }

    /// Create a new operating system.
    pub async fn create(
        &self,
        name: &str,
        version: &str,
        description: Option<&str>,
    ) -> Result<OperatingSystem> {
        let now = Utc::now();

        self.db
            .execute(
                "INSERT INTO operating_systems (name, version, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    name.to_string(),
                    version.to_string(),
                    description.map(|s| s.to_string()),
                    now,
                    now,
                ),
            )
            .await
            .context("Failed to insert operating system")?;

        let id = self.db.last_insert_rowid().await;

        Ok(OperatingSystem {
            id: Some(id),
            name: name.to_string(),
            version: version.to_string(),
            description: description.map(|s| s.to_string()),
            created_at: Some(now),
            updated_at: Some(now),
        })
    }

    /// Get an operating system by ID.
    pub async fn get(&self, id: i64) -> Result<OperatingSystem> {
        let os = self
            .db
            .query_one(
                "SELECT id, name, version, description, created_at, updated_at
                 FROM operating_systems WHERE id = ?1",
                (id,),
                |row| {
                    Ok(OperatingSystem {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        version: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .await
            .context("Operating system not found")?;

        Ok(os)
    }

    /// Get an operating system with all its architecture configurations.
    pub async fn get_with_architectures(
        &self,
        id: i64,
    ) -> Result<OperatingSystemWithArchitectures> {
        let os = self.get(id).await?;
        let architectures = self.list_architectures(id).await?;

        Ok(OperatingSystemWithArchitectures { os, architectures })
    }

    /// List all operating systems.
    pub async fn list(&self) -> Result<Vec<OperatingSystem>> {
        let systems = self
            .db
            .query(
                "SELECT id, name, version, description, created_at, updated_at
                 FROM operating_systems ORDER BY name, version",
                (),
                |row| {
                    Ok(OperatingSystem {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        version: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .await?;

        Ok(systems)
    }

    /// Update an operating system.
    pub async fn update(
        &self,
        id: i64,
        name: Option<&str>,
        version: Option<&str>,
        description: Option<&str>,
    ) -> Result<OperatingSystem> {
        let now = Utc::now();

        let mut updates = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(name) = name {
            updates.push("name = ?");
            values.push(rusqlite::types::Value::Text(name.to_string()));
        }
        if let Some(version) = version {
            updates.push("version = ?");
            values.push(rusqlite::types::Value::Text(version.to_string()));
        }
        if let Some(description) = description {
            updates.push("description = ?");
            values.push(rusqlite::types::Value::Text(description.to_string()));
        }

        if updates.is_empty() {
            return self.get(id).await;
        }

        updates.push("updated_at = ?");
        values.push(rusqlite::types::Value::Text(now.to_rfc3339()));
        values.push(rusqlite::types::Value::Integer(id));

        let query = format!(
            "UPDATE operating_systems SET {} WHERE id = ?",
            updates.join(", ")
        );

        self.db
            .execute(query, rusqlite::params_from_iter(values))
            .await?;

        self.get(id).await
    }

    /// Delete an operating system (and all its architectures due to CASCADE).
    pub async fn delete(&self, id: i64) -> Result<()> {
        let rows_affected = self
            .db
            .execute("DELETE FROM operating_systems WHERE id = ?1", (id,))
            .await
            .context("Failed to delete operating system")?;

        if rows_affected == 0 {
            return Err(anyhow!("Operating system not found"));
        }

        Ok(())
    }

    /// Create or update an architecture configuration for an OS.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_architecture(
        &self,
        os_id: i64,
        architecture: Architecture,
        kernel_path: &str,
        initramfs_path: &str,
        modules: Vec<String>,
        cmdline_args: Option<&str>,
        install_script_path: Option<&str>,
    ) -> Result<OsArchitecture> {
        let now = Utc::now();
        let modules_json = serde_json::to_string(&modules)?;
        let arch_str = architecture.as_str().to_string();

        self.db
            .execute(
                "INSERT INTO os_architectures
                 (os_id, architecture, kernel_path, initramfs_path, modules, cmdline_args, install_script_path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(os_id, architecture) DO UPDATE SET
                 kernel_path = ?3,
                 initramfs_path = ?4,
                 modules = ?5,
                 cmdline_args = ?6,
                 install_script_path = ?7,
                 updated_at = ?9",
                (
                    os_id,
                    arch_str,
                    kernel_path.to_string(),
                    initramfs_path.to_string(),
                    modules_json,
                    cmdline_args.map(|s| s.to_string()),
                    install_script_path.map(|s| s.to_string()),
                    now,
                    now,
                ),
            )
            .await
            .context("Failed to upsert OS architecture")?;

        self.get_architecture(os_id, architecture).await
    }

    /// Get a specific architecture configuration.
    pub async fn get_architecture(
        &self,
        os_id: i64,
        architecture: Architecture,
    ) -> Result<OsArchitecture> {
        let arch_str = architecture.as_str().to_string();

        let arch = self
            .db
            .query_one(
                "SELECT id, os_id, architecture, kernel_path, initramfs_path, modules, cmdline_args, install_script_path, kernel_filename, initramfs_filename, install_script_filename, created_at, updated_at
                 FROM os_architectures WHERE os_id = ?1 AND architecture = ?2",
                (os_id, arch_str),
                move |row| {
                    let modules_json: String = row.get(5)?;
                    let modules: Vec<String> = serde_json::from_str(&modules_json).unwrap_or_default();

                    Ok(OsArchitecture {
                        id: row.get(0)?,
                        os_id: row.get(1)?,
                        architecture,
                        kernel_path: row.get(3)?,
                        initramfs_path: row.get(4)?,
                        modules,
                        cmdline_args: row.get(6)?,
                        install_script_path: row.get(7)?,
                        kernel_filename: row.get(8)?,
                        initramfs_filename: row.get(9)?,
                        install_script_filename: row.get(10)?,
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                    })
                },
            )
            .await
            .context("OS architecture not found")?;

        Ok(arch)
    }

    /// List all architecture configurations for an OS.
    pub async fn list_architectures(&self, os_id: i64) -> Result<Vec<OsArchitecture>> {
        let architectures = self
            .db
            .query(
                "SELECT id, os_id, architecture, kernel_path, initramfs_path, modules, cmdline_args, install_script_path, kernel_filename, initramfs_filename, install_script_filename, created_at, updated_at
                 FROM os_architectures WHERE os_id = ?1 ORDER BY architecture",
                (os_id,),
                |row| {
                    let arch_str: String = row.get(2)?;
                    let architecture = Architecture::from_str(&arch_str).unwrap();
                    let modules_json: String = row.get(5)?;
                    let modules: Vec<String> = serde_json::from_str(&modules_json).unwrap_or_default();

                    Ok(OsArchitecture {
                        id: row.get(0)?,
                        os_id: row.get(1)?,
                        architecture,
                        kernel_path: row.get(3)?,
                        initramfs_path: row.get(4)?,
                        modules,
                        cmdline_args: row.get(6)?,
                        install_script_path: row.get(7)?,
                        kernel_filename: row.get(8)?,
                        initramfs_filename: row.get(9)?,
                        install_script_filename: row.get(10)?,
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                    })
                },
            )
            .await?;

        Ok(architectures)
    }

    /// Update specific fields of an OS architecture.
    pub async fn update_architecture_field(
        &self,
        os_id: i64,
        architecture: Architecture,
        field: &str,
        value: &str,
    ) -> Result<OsArchitecture> {
        let now = Utc::now();
        let arch_str = architecture.as_str().to_string();
        let field = field.to_string();
        let value = value.to_string();

        let query = format!(
            "UPDATE os_architectures SET {} = ?1, updated_at = ?2 WHERE os_id = ?3 AND architecture = ?4",
            field
        );

        self.db
            .execute(query, (value, now, os_id, arch_str))
            .await
            .context("Failed to update OS architecture")?;

        self.get_architecture(os_id, architecture).await
    }

    /// Delete an architecture configuration.
    pub async fn delete_architecture(&self, os_id: i64, architecture: Architecture) -> Result<()> {
        let arch_str = architecture.as_str().to_string();

        let rows_affected = self
            .db
            .execute(
                "DELETE FROM os_architectures WHERE os_id = ?1 AND architecture = ?2",
                (os_id, arch_str),
            )
            .await
            .context("Failed to delete OS architecture")?;

        if rows_affected == 0 {
            return Err(anyhow!("OS architecture not found"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_database_path;

    use super::*;

    async fn setup_db(path: String) -> Arc<Connection> {
        Arc::new(crate::database::open(path).await.unwrap())
    }

    #[tokio::test]
    async fn test_create_and_get_os() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        let os = store
            .create("Ubuntu Server", "24.04", Some("Ubuntu 24.04 LTS"))
            .await
            .unwrap();

        assert!(os.id.is_some());
        assert_eq!(os.name, "Ubuntu Server");
        assert_eq!(os.version, "24.04");

        let retrieved = store.get(os.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, os.name);
    }

    #[tokio::test]
    async fn test_list_os() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        store.create("Ubuntu", "24.04", None).await.unwrap();
        store.create("Debian", "12", None).await.unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_update_os() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).await.unwrap();
        let updated = store
            .update(os.id.unwrap(), None, None, Some("New description"))
            .await
            .unwrap();

        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_os() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).await.unwrap();
        store.delete(os.id.unwrap()).await.unwrap();

        assert!(store.get(os.id.unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn test_upsert_architecture() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).await.unwrap();
        let arch = store
            .upsert_architecture(
                os.id.unwrap(),
                Architecture::X86_64,
                "os/1/kernel",
                "os/1/initramfs",
                vec![],
                Some("console=ttyS0"),
                None,
            )
            .await
            .unwrap();

        assert_eq!(arch.architecture, Architecture::X86_64);
        assert_eq!(arch.kernel_path, "os/1/kernel");

        // Update
        let updated = store
            .upsert_architecture(
                os.id.unwrap(),
                Architecture::X86_64,
                "os/1/kernel-new",
                "os/1/initramfs",
                vec![],
                Some("console=ttyS0"),
                None,
            )
            .await
            .unwrap();

        assert_eq!(updated.kernel_path, "os/1/kernel-new");

        // Should be same ID (updated, not created)
        assert_eq!(arch.id, updated.id);
    }

    #[tokio::test]
    async fn test_list_architectures() {
        let db = setup_db(test_database_path!()).await;
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).await.unwrap();
        store
            .upsert_architecture(
                os.id.unwrap(),
                Architecture::X86_64,
                "kernel",
                "initramfs",
                vec![],
                None,
                None,
            )
            .await
            .unwrap();

        let archs = store.list_architectures(os.id.unwrap()).await.unwrap();
        assert_eq!(archs.len(), 1);
    }
}
