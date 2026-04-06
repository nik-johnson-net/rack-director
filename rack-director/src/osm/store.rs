use anyhow::{Context, Result, anyhow};

use crate::database::Connection;
use osm::os_config::OperatingSystemConfig;

// ── Public types ─────────────────────────────────────────────────────────────

/// A row from `osm_modules`: metadata for an installed OSM archive.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmModule {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    /// Either `"bundled"` or `"uploaded"`.
    pub source: String,
    /// Storage-layer prefix under which the module's files are stored.
    pub storage_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A row from `osm_operating_systems`: one OS entry inside a module.
///
/// The `config` field is deserialized from JSON stored in the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmOperatingSystem {
    pub id: i64,
    pub module_id: i64,
    pub dir_name: String,
    pub name: String,
    pub release: String,
    pub config: OperatingSystemConfig,
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A row from `osm_uploads`: tracks the state of an async upload operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmUpload {
    pub id: i64,
    pub filename: String,
    /// One of: `"uploading"`, `"validating"`, `"extracting"`, `"complete"`, `"failed"`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<i64>,
    pub received_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// ── Module CRUD ───────────────────────────────────────────────────────────────

/// Insert a new OSM module record and return it.
///
/// `source` must be either `"bundled"` or `"uploaded"` (enforced by a DB
/// CHECK constraint).
#[allow(clippy::too_many_arguments)]
pub async fn create_module(
    conn: &Connection,
    name: &str,
    version: &str,
    author: &str,
    description: &str,
    source: &str,
    storage_prefix: &str,
    archive_path: Option<&str>,
) -> Result<OsmModule> {
    conn.execute(
        "INSERT INTO osm_modules (name, version, author, description, source, storage_prefix, archive_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            name.to_string(),
            version.to_string(),
            author.to_string(),
            description.to_string(),
            source.to_string(),
            storage_prefix.to_string(),
            archive_path.map(|s| s.to_string()),
        ),
    )
    .await
    .context("Failed to insert OSM module")?;

    let id = conn.last_insert_rowid().await;
    get_module(conn, id).await
}

/// Fetch a single OSM module by primary key.
pub async fn get_module(conn: &Connection, id: i64) -> Result<OsmModule> {
    conn.query_one(
        "SELECT id, name, version, author, description, source, storage_prefix, archive_path,
                created_at, updated_at
         FROM osm_modules WHERE id = ?1",
        (id,),
        module_from_row,
    )
    .await
    .context("OSM module not found")
}

/// Fetch a single OSM module by name (case-insensitive).
pub async fn get_module_by_name(conn: &Connection, name: &str) -> Result<OsmModule> {
    conn.query_one(
        "SELECT id, name, version, author, description, source, storage_prefix, archive_path,
                created_at, updated_at
         FROM osm_modules WHERE LOWER(name) = LOWER(?1)",
        (name.to_string(),),
        module_from_row,
    )
    .await
    .context("OSM module not found")
}

/// Return all OSM modules sorted by name.
pub async fn list_modules(conn: &Connection) -> Result<Vec<OsmModule>> {
    conn.query(
        "SELECT id, name, version, author, description, source, storage_prefix, archive_path,
                created_at, updated_at
         FROM osm_modules ORDER BY name",
        (),
        module_from_row,
    )
    .await
    .context("Failed to list OSM modules")
}

/// Update the mutable fields of an existing module.
///
/// `name` and `source` are immutable once created — use `delete_module` +
/// `create_module` if those need to change.
pub async fn update_module(
    conn: &Connection,
    id: i64,
    version: &str,
    author: &str,
    description: &str,
    storage_prefix: &str,
    archive_path: Option<&str>,
) -> Result<()> {
    let rows = conn
        .execute(
            "UPDATE osm_modules
             SET version = ?1, author = ?2, description = ?3,
                 storage_prefix = ?4, archive_path = ?5,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?6",
            (
                version.to_string(),
                author.to_string(),
                description.to_string(),
                storage_prefix.to_string(),
                archive_path.map(|s| s.to_string()),
                id,
            ),
        )
        .await
        .context("Failed to update OSM module")?;

    if rows == 0 {
        return Err(anyhow!("OSM module not found"));
    }
    Ok(())
}

/// Delete an OSM module.  All associated `osm_operating_systems` rows are
/// removed automatically via `ON DELETE CASCADE`.
pub async fn delete_module(conn: &Connection, id: i64) -> Result<()> {
    let rows = conn
        .execute("DELETE FROM osm_modules WHERE id = ?1", (id,))
        .await
        .context("Failed to delete OSM module")?;

    if rows == 0 {
        return Err(anyhow!("OSM module not found"));
    }
    Ok(())
}

// ── OS CRUD ───────────────────────────────────────────────────────────────────

/// Insert an OS entry for a module.
///
/// `config` is serialized to JSON for storage.
pub async fn create_operating_system(
    conn: &Connection,
    module_id: i64,
    dir_name: &str,
    name: &str,
    release: &str,
    config: &OperatingSystemConfig,
) -> Result<OsmOperatingSystem> {
    let config_json = serde_json::to_string(config).context("Failed to serialize OS config")?;

    conn.execute(
        "INSERT INTO osm_operating_systems (module_id, dir_name, name, release, config)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            module_id,
            dir_name.to_string(),
            name.to_string(),
            release.to_string(),
            config_json,
        ),
    )
    .await
    .context("Failed to insert OSM operating system")?;

    let id = conn.last_insert_rowid().await;
    get_operating_system_by_id(conn, id).await
}

/// List all OS entries belonging to a module.
pub async fn list_operating_systems(
    conn: &Connection,
    module_id: i64,
) -> Result<Vec<OsmOperatingSystem>> {
    conn.query(
        "SELECT id, module_id, dir_name, name, release, config, disabled, created_at, updated_at
         FROM osm_operating_systems WHERE module_id = ?1 ORDER BY dir_name",
        (module_id,),
        os_from_row,
    )
    .await
    .context("Failed to list OSM operating systems")
}

/// List all OS entries across all modules.
pub async fn list_all_operating_systems(conn: &Connection) -> Result<Vec<OsmOperatingSystem>> {
    conn.query(
        "SELECT id, module_id, dir_name, name, release, config, disabled, created_at, updated_at
         FROM osm_operating_systems ORDER BY module_id, dir_name",
        (),
        os_from_row,
    )
    .await
    .context("Failed to list all OSM operating systems")
}

/// Fetch a single OS entry by module + directory name.
pub async fn get_operating_system(
    conn: &Connection,
    module_id: i64,
    dir_name: &str,
) -> Result<OsmOperatingSystem> {
    conn.query_one(
        "SELECT id, module_id, dir_name, name, release, config, disabled, created_at, updated_at
         FROM osm_operating_systems WHERE module_id = ?1 AND dir_name = ?2",
        (module_id, dir_name.to_string()),
        os_from_row,
    )
    .await
    .context("OSM operating system not found")
}

/// Enable or disable an OS entry.
pub async fn set_os_disabled(conn: &Connection, os_id: i64, disabled: bool) -> Result<()> {
    let rows = conn
        .execute(
            "UPDATE osm_operating_systems
             SET disabled = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            (disabled as i32, os_id),
        )
        .await
        .context("Failed to update OS disabled flag")?;

    if rows == 0 {
        return Err(anyhow!("OSM operating system not found"));
    }
    Ok(())
}

/// Delete all OS entries for a module.
///
/// This is a convenience helper used before re-importing a module; the normal
/// deletion path goes through `delete_module` which cascades automatically.
pub async fn delete_operating_systems_for_module(conn: &Connection, module_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM osm_operating_systems WHERE module_id = ?1",
        (module_id,),
    )
    .await
    .context("Failed to delete OSM operating systems for module")?;

    Ok(())
}

// ── Upload tracking ───────────────────────────────────────────────────────────

/// Create an upload tracking record.
///
/// `total_bytes` may be `None` when the `Content-Length` header is absent.
pub async fn create_upload(
    conn: &Connection,
    filename: &str,
    total_bytes: Option<i64>,
) -> Result<OsmUpload> {
    conn.execute(
        "INSERT INTO osm_uploads (filename, status, total_bytes)
         VALUES (?1, 'uploading', ?2)",
        (filename.to_string(), total_bytes),
    )
    .await
    .context("Failed to create OSM upload")?;

    let id = conn.last_insert_rowid().await;
    get_upload(conn, id).await
}

/// Update the status of an upload, optionally recording an error message and
/// the ID of the module that was created (on success).
pub async fn update_upload_status(
    conn: &Connection,
    upload_id: i64,
    status: &str,
    error_message: Option<&str>,
    module_id: Option<i64>,
) -> Result<()> {
    let rows = conn
        .execute(
            "UPDATE osm_uploads
             SET status = ?1, error_message = ?2, module_id = ?3,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?4",
            (
                status.to_string(),
                error_message.map(|s| s.to_string()),
                module_id,
                upload_id,
            ),
        )
        .await
        .context("Failed to update OSM upload status")?;

    if rows == 0 {
        return Err(anyhow!("OSM upload not found"));
    }
    Ok(())
}

/// Record the number of bytes received so far for a streaming upload.
pub async fn update_upload_progress(
    conn: &Connection,
    upload_id: i64,
    received_bytes: i64,
) -> Result<()> {
    let rows = conn
        .execute(
            "UPDATE osm_uploads
             SET received_bytes = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            (received_bytes, upload_id),
        )
        .await
        .context("Failed to update OSM upload progress")?;

    if rows == 0 {
        return Err(anyhow!("OSM upload not found"));
    }
    Ok(())
}

/// Return the 50 most recent upload records, newest first.
pub async fn list_uploads(conn: &Connection) -> Result<Vec<OsmUpload>> {
    conn.query(
        "SELECT id, filename, status, error_message, module_id, total_bytes, received_bytes,
                created_at, updated_at
         FROM osm_uploads ORDER BY created_at DESC, id DESC LIMIT 50",
        (),
        upload_from_row,
    )
    .await
    .context("Failed to list OSM uploads")
}

/// Fetch a single upload record by primary key.
pub async fn get_upload(conn: &Connection, upload_id: i64) -> Result<OsmUpload> {
    conn.query_one(
        "SELECT id, filename, status, error_message, module_id, total_bytes, received_bytes,
                created_at, updated_at
         FROM osm_uploads WHERE id = ?1",
        (upload_id,),
        upload_from_row,
    )
    .await
    .context("OSM upload not found")
}

// ── Private row-mapping helpers ───────────────────────────────────────────────

fn module_from_row(row: &rusqlite::Row) -> rusqlite::Result<OsmModule> {
    Ok(OsmModule {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        author: row.get(3)?,
        description: row.get(4)?,
        source: row.get(5)?,
        storage_prefix: row.get(6)?,
        archive_path: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn os_from_row(row: &rusqlite::Row) -> rusqlite::Result<OsmOperatingSystem> {
    let config_json: String = row.get(5)?;
    let config: OperatingSystemConfig = serde_json::from_str(&config_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let disabled_int: i32 = row.get(6)?;

    Ok(OsmOperatingSystem {
        id: row.get(0)?,
        module_id: row.get(1)?,
        dir_name: row.get(2)?,
        name: row.get(3)?,
        release: row.get(4)?,
        config,
        disabled: disabled_int != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn upload_from_row(row: &rusqlite::Row) -> rusqlite::Result<OsmUpload> {
    Ok(OsmUpload {
        id: row.get(0)?,
        filename: row.get(1)?,
        status: row.get(2)?,
        error_message: row.get(3)?,
        module_id: row.get(4)?,
        total_bytes: row.get(5)?,
        received_bytes: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// Fetch a single OS entry by its primary key (internal helper).
async fn get_operating_system_by_id(conn: &Connection, id: i64) -> Result<OsmOperatingSystem> {
    conn.query_one(
        "SELECT id, module_id, dir_name, name, release, config, disabled, created_at, updated_at
         FROM osm_operating_systems WHERE id = ?1",
        (id,),
        os_from_row,
    )
    .await
    .context("OSM operating system not found")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, test_connection_factory};
    use osm::os_config::{ArchitectureConfig, OperatingSystemConfig};

    fn sample_config() -> OperatingSystemConfig {
        OperatingSystemConfig {
            name: "Test OS".to_string(),
            release: "1.0".to_string(),
            architectures: vec![ArchitectureConfig {
                arch: "x86-64".to_string(),
                kernel: "vmlinuz".to_string(),
                initramfs: "initrd.img".to_string(),
                modules: vec![],
                cmdline: String::new(),
                install_template: "install.sh".to_string(),
            }],
            template_variables: vec![],
        }
    }

    // ── Module tests ──────────────────────────────────────────────────────────

    /// Create a module and verify it can be retrieved by ID.
    #[tokio::test]
    async fn test_create_and_get_module() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let module = create_module(
            &conn,
            "ubuntu",
            "1.0.0",
            "Rack Team",
            "Ubuntu OSM",
            "bundled",
            "osm/ubuntu",
            None,
        )
        .await
        .unwrap();

        assert!(module.id > 0);
        assert_eq!(module.name, "ubuntu");
        assert_eq!(module.version, "1.0.0");
        assert_eq!(module.source, "bundled");
        assert!(module.archive_path.is_none());

        let fetched = get_module(&conn, module.id).await.unwrap();
        assert_eq!(fetched.name, module.name);
        assert_eq!(fetched.version, module.version);
    }

    /// `get_module_by_name` must match regardless of the case supplied by the caller.
    #[tokio::test]
    async fn test_get_module_by_name_case_insensitive() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        create_module(
            &conn, "Ubuntu", "1.0", "Team", "desc", "bundled", "prefix", None,
        )
        .await
        .unwrap();

        let lower = get_module_by_name(&conn, "ubuntu").await.unwrap();
        assert_eq!(lower.name, "Ubuntu");

        let upper = get_module_by_name(&conn, "UBUNTU").await.unwrap();
        assert_eq!(upper.name, "Ubuntu");
    }

    /// `list_modules` returns all modules sorted by name.
    #[tokio::test]
    async fn test_list_modules_sorted_by_name() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        create_module(&conn, "zebra", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();
        create_module(&conn, "alpha", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        let modules = list_modules(&conn).await.unwrap();
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].name, "alpha");
        assert_eq!(modules[1].name, "zebra");
    }

    /// Deleting a module must cascade and remove its OS entries.
    #[tokio::test]
    async fn test_delete_module_cascades_to_os() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let module = create_module(&conn, "mod", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        create_operating_system(&conn, module.id, "os1", "OS One", "1.0", &sample_config())
            .await
            .unwrap();

        let before = list_operating_systems(&conn, module.id).await.unwrap();
        assert_eq!(before.len(), 1);

        delete_module(&conn, module.id).await.unwrap();

        // The module is gone.
        assert!(get_module(&conn, module.id).await.is_err());

        // The OS rows are also gone (cascade).
        let after = list_operating_systems(&conn, module.id).await.unwrap();
        assert!(after.is_empty());
    }

    /// Duplicate module names must be rejected with a unique-constraint error.
    #[tokio::test]
    async fn test_duplicate_module_name_fails() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        create_module(&conn, "dup", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        let result = create_module(&conn, "dup", "2", "B", "e", "uploaded", "q", None).await;
        assert!(result.is_err());
    }

    // ── OS tests ──────────────────────────────────────────────────────────────

    /// Create an OS entry and verify the config round-trips through JSON.
    #[tokio::test]
    async fn test_create_and_list_operating_systems_with_config_roundtrip() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let module = create_module(&conn, "mod", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        let config = sample_config();
        let os = create_operating_system(&conn, module.id, "test-os", "Test OS", "1.0", &config)
            .await
            .unwrap();

        assert!(os.id > 0);
        assert_eq!(os.module_id, module.id);
        assert_eq!(os.dir_name, "test-os");
        assert!(!os.disabled);
        assert_eq!(os.config, config);

        let list = list_operating_systems(&conn, module.id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config, config);
    }

    /// `set_os_disabled` must toggle the disabled flag correctly.
    #[tokio::test]
    async fn test_set_os_disabled() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let module = create_module(&conn, "mod", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        let os = create_operating_system(&conn, module.id, "os1", "OS", "1.0", &sample_config())
            .await
            .unwrap();

        assert!(!os.disabled);

        set_os_disabled(&conn, os.id, true).await.unwrap();
        let fetched = get_operating_system(&conn, module.id, "os1").await.unwrap();
        assert!(fetched.disabled);

        set_os_disabled(&conn, os.id, false).await.unwrap();
        let fetched = get_operating_system(&conn, module.id, "os1").await.unwrap();
        assert!(!fetched.disabled);
    }

    /// `set_os_disabled` on a non-existent ID returns an error.
    #[tokio::test]
    async fn test_set_os_disabled_not_found() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let result = set_os_disabled(&conn, 9999, true).await;
        assert!(result.is_err());
    }

    // ── Upload tests ──────────────────────────────────────────────────────────

    /// Create an upload, update its progress, then mark it complete.
    #[tokio::test]
    async fn test_upload_tracking_lifecycle() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let upload = create_upload(&conn, "module.tar.gz", Some(1024))
            .await
            .unwrap();
        assert_eq!(upload.status, "uploading");
        assert_eq!(upload.received_bytes, 0);
        assert_eq!(upload.total_bytes, Some(1024));

        update_upload_progress(&conn, upload.id, 512).await.unwrap();
        let mid = get_upload(&conn, upload.id).await.unwrap();
        assert_eq!(mid.received_bytes, 512);

        // Create a module so we can link it.
        let module = create_module(&conn, "mod", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        update_upload_status(&conn, upload.id, "complete", None, Some(module.id))
            .await
            .unwrap();

        let done = get_upload(&conn, upload.id).await.unwrap();
        assert_eq!(done.status, "complete");
        assert_eq!(done.module_id, Some(module.id));
        assert!(done.error_message.is_none());
    }

    /// A failed upload stores an error message.
    #[tokio::test]
    async fn test_upload_failed_with_error_message() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let upload = create_upload(&conn, "bad.tar.gz", None).await.unwrap();

        update_upload_status(&conn, upload.id, "failed", Some("archive is corrupt"), None)
            .await
            .unwrap();

        let failed = get_upload(&conn, upload.id).await.unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_message.as_deref(), Some("archive is corrupt"));
        assert!(failed.module_id.is_none());
    }

    /// `list_uploads` returns records newest-first (by id DESC as stable tie-breaker),
    /// capped at 50.
    #[tokio::test]
    async fn test_list_uploads() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        create_upload(&conn, "first.tar.gz", None).await.unwrap();
        create_upload(&conn, "second.tar.gz", None).await.unwrap();

        let uploads = list_uploads(&conn).await.unwrap();
        assert_eq!(uploads.len(), 2);
        // Ordered by (created_at DESC, id DESC) — "second" always has a higher id,
        // so it appears first regardless of timestamp resolution.
        assert_eq!(uploads[0].filename, "second.tar.gz");
        assert_eq!(uploads[1].filename, "first.tar.gz");
    }

    /// `update_upload_status` on a missing ID returns an error.
    #[tokio::test]
    async fn test_update_upload_status_not_found() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let result = update_upload_status(&conn, 9999, "complete", None, None).await;
        assert!(result.is_err());
    }

    /// `update_module` on a non-existent ID returns an error.
    #[tokio::test]
    async fn test_update_module_not_found() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let result = update_module(&conn, 9999, "2.0", "Author", "desc", "prefix", None).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not found"), "unexpected error: {msg}");
    }

    /// `delete_operating_systems_for_module` removes all OS entries but leaves the module intact.
    #[tokio::test]
    async fn test_delete_operating_systems_for_module() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let module = create_module(&conn, "mod", "1", "A", "d", "bundled", "p", None)
            .await
            .unwrap();

        create_operating_system(&conn, module.id, "os1", "OS One", "1.0", &sample_config())
            .await
            .unwrap();
        create_operating_system(&conn, module.id, "os2", "OS Two", "2.0", &sample_config())
            .await
            .unwrap();

        let before = list_operating_systems(&conn, module.id).await.unwrap();
        assert_eq!(before.len(), 2);

        delete_operating_systems_for_module(&conn, module.id)
            .await
            .unwrap();

        // OS entries are gone.
        let after = list_operating_systems(&conn, module.id).await.unwrap();
        assert!(after.is_empty());

        // Module still exists.
        let module_after = get_module(&conn, module.id).await;
        assert!(
            module_after.is_ok(),
            "module should still exist after deleting its OS entries"
        );
    }

    /// `list_all_operating_systems` returns OS entries from all modules.
    #[tokio::test]
    async fn test_list_all_operating_systems() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let mod1 = create_module(&conn, "mod1", "1", "A", "d", "bundled", "p1", None)
            .await
            .unwrap();
        let mod2 = create_module(&conn, "mod2", "1", "A", "d", "bundled", "p2", None)
            .await
            .unwrap();

        create_operating_system(&conn, mod1.id, "os1", "OS One", "1.0", &sample_config())
            .await
            .unwrap();
        create_operating_system(&conn, mod2.id, "os2", "OS Two", "2.0", &sample_config())
            .await
            .unwrap();

        let all = list_all_operating_systems(&conn).await.unwrap();
        assert_eq!(all.len(), 2, "expected 2 OS entries across all modules");

        // Both module IDs appear.
        let module_ids: Vec<i64> = all.iter().map(|os| os.module_id).collect();
        assert!(module_ids.contains(&mod1.id));
        assert!(module_ids.contains(&mod2.id));
    }
}
