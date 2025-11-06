use super::{Architecture, OperatingSystem, OsArchitecture, OperatingSystemWithArchitectures};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

pub struct OperatingSystemsStore {
    db: Arc<Mutex<Connection>>,
}

impl OperatingSystemsStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Create a new operating system
    pub fn create(&self, name: &str, version: &str, description: Option<&str>) -> Result<OperatingSystem> {
        let conn = self.db.lock().unwrap();
        let now = Utc::now();

        conn.execute(
            "INSERT INTO operating_systems (name, version, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, version, description, now, now],
        )
        .context("Failed to insert operating system")?;

        let id = conn.last_insert_rowid();

        Ok(OperatingSystem {
            id: Some(id),
            name: name.to_string(),
            version: version.to_string(),
            description: description.map(|s| s.to_string()),
            created_at: Some(now),
            updated_at: Some(now),
        })
    }

    /// Get an operating system by ID
    pub fn get(&self, id: i64) -> Result<OperatingSystem> {
        let conn = self.db.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, name, version, description, created_at, updated_at
             FROM operating_systems WHERE id = ?1",
        )?;

        let os = stmt
            .query_row(params![id], |row| {
                Ok(OperatingSystem {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    version: row.get(2)?,
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .context("Operating system not found")?;

        Ok(os)
    }

    /// Get an operating system with all its architecture configurations
    pub fn get_with_architectures(&self, id: i64) -> Result<OperatingSystemWithArchitectures> {
        let os = self.get(id)?;
        let architectures = self.list_architectures(id)?;

        Ok(OperatingSystemWithArchitectures { os, architectures })
    }

    /// List all operating systems
    pub fn list(&self) -> Result<Vec<OperatingSystem>> {
        let conn = self.db.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, name, version, description, created_at, updated_at
             FROM operating_systems ORDER BY name, version",
        )?;

        let rows = stmt.query_map(params![], |row| {
            Ok(OperatingSystem {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                description: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;

        let mut systems = Vec::new();
        for row in rows {
            systems.push(row?);
        }

        Ok(systems)
    }

    /// Update an operating system
    pub fn update(
        &self,
        id: i64,
        name: Option<&str>,
        version: Option<&str>,
        description: Option<&str>,
    ) -> Result<OperatingSystem> {
        let conn = self.db.lock().unwrap();
        let now = Utc::now();

        // Build dynamic update query
        let mut updates = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(name) = name {
            updates.push("name = ?");
            params.push(Box::new(name.to_string()));
        }
        if let Some(version) = version {
            updates.push("version = ?");
            params.push(Box::new(version.to_string()));
        }
        if let Some(description) = description {
            updates.push("description = ?");
            params.push(Box::new(description.to_string()));
        }

        if updates.is_empty() {
            return self.get(id);
        }

        updates.push("updated_at = ?");
        params.push(Box::new(now));
        params.push(Box::new(id));

        let query = format!(
            "UPDATE operating_systems SET {} WHERE id = ?",
            updates.join(", ")
        );

        conn.execute(&query, rusqlite::params_from_iter(params.iter()))?;

        self.get(id)
    }

    /// Delete an operating system (and all its architectures due to CASCADE)
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.db.lock().unwrap();

        let rows_affected = conn
            .execute("DELETE FROM operating_systems WHERE id = ?1", params![id])
            .context("Failed to delete operating system")?;

        if rows_affected == 0 {
            return Err(anyhow!("Operating system not found"));
        }

        Ok(())
    }

    /// Create or update an architecture configuration for an OS
    pub fn upsert_architecture(
        &self,
        os_id: i64,
        architecture: Architecture,
        kernel_path: &str,
        initramfs_path: &str,
        modules: Vec<String>,
        cmdline_args: Option<&str>,
        install_script_path: Option<&str>,
    ) -> Result<OsArchitecture> {
        let conn = self.db.lock().unwrap();
        let now = Utc::now();
        let modules_json = serde_json::to_string(&modules)?;
        let arch_str = architecture.as_str();

        conn.execute(
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
            params![
                os_id,
                arch_str,
                kernel_path,
                initramfs_path,
                modules_json,
                cmdline_args,
                install_script_path,
                now,
                now
            ],
        )
        .context("Failed to upsert OS architecture")?;

        self.get_architecture(os_id, architecture)
    }

    /// Get a specific architecture configuration
    pub fn get_architecture(&self, os_id: i64, architecture: Architecture) -> Result<OsArchitecture> {
        let conn = self.db.lock().unwrap();
        let arch_str = architecture.as_str();

        let mut stmt = conn.prepare(
            "SELECT id, os_id, architecture, kernel_path, initramfs_path, modules, cmdline_args, install_script_path, created_at, updated_at
             FROM os_architectures WHERE os_id = ?1 AND architecture = ?2",
        )?;

        let arch = stmt
            .query_row(params![os_id, arch_str], |row| {
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
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .context("OS architecture not found")?;

        Ok(arch)
    }

    /// List all architecture configurations for an OS
    pub fn list_architectures(&self, os_id: i64) -> Result<Vec<OsArchitecture>> {
        let conn = self.db.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, os_id, architecture, kernel_path, initramfs_path, modules, cmdline_args, install_script_path, created_at, updated_at
             FROM os_architectures WHERE os_id = ?1 ORDER BY architecture",
        )?;

        let rows = stmt.query_map(params![os_id], |row| {
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
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;

        let mut architectures = Vec::new();
        for row in rows {
            architectures.push(row?);
        }

        Ok(architectures)
    }

    /// Update specific fields of an OS architecture
    pub fn update_architecture_field(
        &self,
        os_id: i64,
        architecture: Architecture,
        field: &str,
        value: &str,
    ) -> Result<OsArchitecture> {
        let conn = self.db.lock().unwrap();
        let now = Utc::now();
        let arch_str = architecture.as_str();

        let query = format!(
            "UPDATE os_architectures SET {} = ?1, updated_at = ?2 WHERE os_id = ?3 AND architecture = ?4",
            field
        );

        conn.execute(&query, params![value, now, os_id, arch_str])
            .context("Failed to update OS architecture")?;

        self.get_architecture(os_id, architecture)
    }

    /// Delete an architecture configuration
    pub fn delete_architecture(&self, os_id: i64, architecture: Architecture) -> Result<()> {
        let conn = self.db.lock().unwrap();
        let arch_str = architecture.as_str();

        let rows_affected = conn
            .execute(
                "DELETE FROM os_architectures WHERE os_id = ?1 AND architecture = ?2",
                params![os_id, arch_str],
            )
            .context("Failed to delete OS architecture")?;

        if rows_affected == 0 {
            return Err(anyhow!("OS architecture not found"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;

    fn setup_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        database::run_migrations(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn test_create_and_get_os() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        let os = store
            .create("Ubuntu Server", "24.04", Some("Ubuntu 24.04 LTS"))
            .unwrap();

        assert!(os.id.is_some());
        assert_eq!(os.name, "Ubuntu Server");
        assert_eq!(os.version, "24.04");

        let retrieved = store.get(os.id.unwrap()).unwrap();
        assert_eq!(retrieved.name, os.name);
    }

    #[test]
    fn test_list_os() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        store.create("Ubuntu", "24.04", None).unwrap();
        store.create("Debian", "12", None).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_update_os() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).unwrap();
        let updated = store
            .update(os.id.unwrap(), None, None, Some("New description"))
            .unwrap();

        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[test]
    fn test_delete_os() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).unwrap();
        store.delete(os.id.unwrap()).unwrap();

        assert!(store.get(os.id.unwrap()).is_err());
    }

    #[test]
    fn test_upsert_architecture() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).unwrap();
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
            .unwrap();

        assert_eq!(updated.kernel_path, "os/1/kernel-new");

        // Should be same ID (updated, not created)
        assert_eq!(arch.id, updated.id);
    }

    #[test]
    fn test_list_architectures() {
        let db = setup_db();
        let store = OperatingSystemsStore::new(db);

        let os = store.create("Ubuntu", "24.04", None).unwrap();
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
            .unwrap();

        let archs = store.list_architectures(os.id.unwrap()).unwrap();
        assert_eq!(archs.len(), 1);
    }
}
