mod connection;
mod migrations;

use anyhow::Result;
pub use connection::Connection;

/// A factory for opening database connections.
///
/// Implementors produce independent `Connection` instances on demand,
/// allowing callers to open a fresh connection per request or operation
/// without sharing a single connection across concurrent tasks.
#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync {
    /// Open a new database connection.
    async fn open(&self) -> Result<Connection>;
}

/// A `ConnectionFactory` backed by a filesystem path to a SQLite database.
///
/// Each call to `open` returns an independent connection with no shared state.
/// Migrations are NOT run on each open call — callers must invoke
/// `database::run_migrations` once at startup before opening connections
/// for normal use.
pub struct DatabaseConnectionFactory {
    path: std::path::PathBuf,
}

impl DatabaseConnectionFactory {
    /// Create a new factory that opens connections to the SQLite database at `path`.
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait::async_trait]
impl ConnectionFactory for DatabaseConnectionFactory {
    async fn open(&self) -> Result<Connection> {
        Connection::open(self.path.clone()).await
    }
}

/// Macro to create a `DatabaseConnectionFactory` pointing at this test's in-memory database.
///
/// Must be called at the top level of the test (same scoping rules as `test_database_path!`).
/// Call `database::run_migrations(&factory).await` before opening connections to ensure the
/// schema is up to date.
#[cfg(test)]
#[macro_export]
macro_rules! test_connection_factory {
    () => {
        crate::database::DatabaseConnectionFactory::new(std::path::PathBuf::from(
            crate::test_database_path!(),
        ))
    };
}

/// Trait for types that can be constructed from a database row.
/// Provides a standard interface for row-to-struct conversion.
pub trait FromRow: Sized {
    /// Construct an instance of Self from a rusqlite Row.
    ///
    /// This method should use named column access (e.g., `row.get("column_name")`)
    /// rather than positional access for better maintainability and robustness.
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

const LATEST_VERSION: usize = 17;
const MIGRATIONS: [&str; LATEST_VERSION] = [
    include_str!("migrations/1.sql"),
    include_str!("migrations/2.sql"),
    include_str!("migrations/3.sql"),
    include_str!("migrations/4.sql"),
    include_str!("migrations/5.sql"),
    include_str!("migrations/6.sql"),
    include_str!("migrations/7.sql"),
    include_str!("migrations/8.sql"),
    include_str!("migrations/9.sql"),
    include_str!("migrations/10.sql"),
    include_str!("migrations/11.sql"),
    include_str!("migrations/12.sql"),
    include_str!("migrations/13.sql"),
    include_str!("migrations/14.sql"),
    include_str!("migrations/15.sql"),
    include_str!("migrations/16.sql"),
    include_str!("migrations/17.sql"),
];

use futures::{FutureExt, future::BoxFuture};

/// Post-migration hooks that run Rust code after SQL migrations
/// Index corresponds to migration version (1-indexed, so POST_MIGRATION_HOOKS[10] runs after migration 11)
type PostMigrationHook = fn(&Connection) -> BoxFuture<Result<()>>;
const POST_MIGRATION_HOOKS: [Option<PostMigrationHook>; LATEST_VERSION] = [
    None,                                                                      // Migration 1
    None,                                                                      // Migration 2
    None,                                                                      // Migration 3
    None,                                                                      // Migration 4
    None,                                                                      // Migration 5
    None,                                                                      // Migration 6
    None,                                                                      // Migration 7
    None,                                                                      // Migration 8
    None,                                                                      // Migration 9
    None,                                                                      // Migration 10
    Some(|conn| migrations::migration_11::convert_uuids(conn).boxed()),        // Migration 11
    None,                                                                      // Migration 12
    None,                                                                      // Migration 13
    Some(|conn| migrations::migration_14::convert_disk_layouts(conn).boxed()), // Migration 14
    Some(|conn| migrations::migration_15::strip_disk_paths(conn).boxed()),     // Migration 15
    None,                                                                      // Migration 16
    None,                                                                      // Migration 17
];

/// Run all pending database migrations against the database opened by `factory`.
///
/// This function opens a single connection via the factory, applies any SQL migrations that
/// have not yet been applied, runs any associated post-migration hooks, and performs
/// a one-time bad-data cleanup. The migrated connection is returned so that callers can
/// hold it open — this is important for in-memory SQLite databases, which are destroyed
/// when all connections close.
///
/// Callers should invoke this once at startup before opening any other connections so that
/// the schema is fully initialised before concurrent factory calls begin.
pub async fn run_migrations(factory: &dyn ConnectionFactory) -> Result<Connection> {
    let mut conn = factory.open().await?;
    let current_version = get_or_init_current_migration(&mut conn).await?;
    perform_migrations(&mut conn, current_version).await?;

    // Hack for bad data from Bug #1
    // Note: After migration 11, UUIDs are BLOBs
    // The migration should have already removed this bad data, but we keep this as a safeguard
    conn.execute("DELETE FROM devices WHERE uuid = x'7b757569647d'", ())
        .await
        .unwrap();

    Ok(conn)
}

async fn get_or_init_current_migration(conn: &mut Connection) -> Result<usize> {
    log::debug!("Checking for migrations");

    if conn.table_exists("migrations").await? {
        let version = conn
            .query_one("SELECT version FROM migrations", [], |r| r.get(0))
            .await?;
        Ok(version)
    } else {
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
                  INSERT INTO migrations (version) VALUES (0)",
        )
        .await?;
        Ok(0)
    }
}

async fn perform_migrations(conn: &mut Connection, current_version: usize) -> Result<()> {
    let mut version = current_version;
    while version < LATEST_VERSION {
        version += 1;

        let tx = conn.transaction().await?;

        if let Err(e) = perform_migration(&tx, version).await {
            tx.rollback().await?;
            return Err(e);
        }

        tx.commit().await?;
    }

    Ok(())
}

async fn perform_migration(conn: &Connection, version: usize) -> Result<()> {
    // Run SQL migration
    if let Err(e) = conn.execute_batch(MIGRATIONS[version - 1]).await {
        log::error!("Couldn't update database. {e}");
        return Err(e.into());
    }

    // Run post-migration hook if it exists
    if let Some(ref hook) = POST_MIGRATION_HOOKS[version - 1] {
        log::debug!("Running post-migration hook for version {}", version);
        hook(conn).await?;
    }

    conn.execute("UPDATE migrations SET version = ?1 ", [version])
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_database_path;

    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_migrations() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let factory = DatabaseConnectionFactory::new(db_path);
        assert!(run_migrations(&factory).await.is_ok());
    }

    #[tokio::test]
    async fn test_database_schema() {
        let factory = test_connection_factory!();
        let conn = run_migrations(&factory).await.unwrap();

        // Test that tables exist
        let table_names: Vec<String> = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table'",
                (),
                |row| row.get(0),
            )
            .await
            .unwrap();

        assert!(table_names.contains(&"devices".to_string()));
        assert!(table_names.contains(&"plans".to_string()));
        assert!(table_names.contains(&"lifecycle_transitions".to_string()));
        assert!(table_names.contains(&"migrations".to_string()));
    }

    #[tokio::test]
    async fn test_device_operations() {
        use uuid::Uuid;

        let factory = test_connection_factory!();
        let conn = run_migrations(&factory).await.unwrap();

        // Test creating a device with BLOB UUID
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle) VALUES (?1, 'new')",
            (test_uuid,),
        )
        .await
        .unwrap();

        // Test querying the device
        let retrieved_uuid: Uuid = conn
            .query_one(
                "SELECT uuid FROM devices WHERE uuid = ?1",
                (test_uuid,),
                |row| row.get(0),
            )
            .await
            .unwrap();

        assert_eq!(retrieved_uuid, test_uuid);
    }

    #[tokio::test]
    async fn test_migration_11_uuid_conversion() {
        use uuid::Uuid;

        let conn = Connection::open(test_database_path!()).await.unwrap();

        // Set up database to migration 10 (before UUID BLOB migration)
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations (version) VALUES (0)",
        )
        .await
        .unwrap();

        // Run migrations 1-10
        for version in 1..=10 {
            conn.execute_batch(MIGRATIONS[version - 1]).await.unwrap();
            conn.execute("UPDATE migrations SET version = ?1", [version])
                .await
                .unwrap();
        }

        // Insert test data with TEXT UUIDs
        let test_uuid1 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let test_uuid2 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (test_uuid1.to_string(),),
        )
        .await
        .unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (test_uuid2.to_string(),),
        )
        .await
        .unwrap();

        // Insert plan with TEXT device_uuid
        conn.execute(
            "INSERT INTO plans (device_uuid, status, total_steps, actions) VALUES (?1, 'pending', 1, '[]')",
            (test_uuid1.to_string(),),
        ).await
        .unwrap();

        // Verify UUIDs are stored as TEXT before migration
        let uuid_type: String = conn
            .query_row(
                "SELECT typeof(uuid) FROM devices WHERE uuid = ?1",
                (test_uuid1.to_string(),),
                |row| row.get(0),
            )
            .await
            .unwrap();
        assert_eq!(uuid_type, "text");

        // Run migration 11
        let factory2 = test_connection_factory!();
        let conn2 = run_migrations(&factory2).await.unwrap();

        // Verify UUIDs are now stored as BLOB
        let uuid_type: String = conn2
            .query_row("SELECT typeof(uuid) FROM devices LIMIT 1", [], |row| {
                row.get(0)
            })
            .await
            .unwrap();
        assert_eq!(uuid_type, "blob");

        // Verify we can read UUIDs as Uuid type
        let uuids: Vec<Uuid> = conn2
            .query("SELECT uuid FROM devices ORDER BY uuid", (), |row| {
                row.get(0)
            })
            .await
            .unwrap();

        assert_eq!(uuids.len(), 2);
        assert!(uuids.contains(&test_uuid1));
        assert!(uuids.contains(&test_uuid2));

        // Verify plan device_uuid was also converted
        let plan_uuid: Uuid = conn2
            .query_row("SELECT device_uuid FROM plans", [], |row| row.get(0))
            .await
            .unwrap();
        assert_eq!(plan_uuid, test_uuid1);
    }

    #[tokio::test]
    async fn test_migration_14_disk_layout_conversion() {
        let conn = Connection::open(test_database_path!()).await.unwrap();

        // Set up database to migration 13 (before disk layout migration)
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations (version) VALUES (0)",
        )
        .await
        .unwrap();

        // Run migrations 1-13
        for version in 1..=13 {
            conn.execute_batch(MIGRATIONS[version - 1]).await.unwrap();
            if let Some(hook) = POST_MIGRATION_HOOKS[version - 1] {
                hook(&conn).await.unwrap();
            }
            conn.execute("UPDATE migrations SET version = ?1", [version])
                .await
                .unwrap();
        }

        // Insert test OS (required by roles foreign key)
        conn.execute(
            "INSERT INTO operating_systems (name, version) VALUES ('TestOS', '1.0')",
            [],
        )
        .await
        .unwrap();

        // Insert test role with old disk layout format
        let old_layout = r#"{
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
        }"#;

        conn.execute(
            "INSERT INTO roles (name, os_id, disk_layout) VALUES ('test_role', 1, ?1)",
            (old_layout,),
        )
        .await
        .unwrap();

        // Verify old format is stored
        let stored: String = conn
            .query_row(
                "SELECT disk_layout FROM roles WHERE name = 'test_role'",
                [],
                |row| row.get(0),
            )
            .await
            .unwrap();
        assert!(stored.contains("\"partitions\""));
        assert!(!stored.contains("\"disks\""));

        // Run migration 14
        let conn_factory = test_connection_factory!();
        run_migrations(&conn_factory).await.unwrap();

        // Verify new format is stored
        let stored: String = conn
            .query_row(
                "SELECT disk_layout FROM roles WHERE name = 'test_role'",
                [],
                |row| row.get(0),
            )
            .await
            .unwrap();
        // The new format should have "disks" at the top level
        assert!(stored.contains("\"disks\""));
        // The old format had top-level "partitions" - this should be gone
        // (Note: "partitions" still exists inside each disk in the new format)
        let layout: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert!(layout.get("disks").is_some());
        assert!(layout.get("partitions").is_none());

        // Verify structure is correct
        let layout: serde_json::Value = serde_json::from_str(&stored).unwrap();
        let disks = layout.get("disks").unwrap().as_array().unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(
            disks[0].get("device").unwrap().as_str().unwrap(),
            "/dev/sda"
        );
        assert_eq!(
            disks[0].get("partition_table").unwrap().as_str().unwrap(),
            "gpt"
        );

        let partitions = disks[0].get("partitions").unwrap().as_array().unwrap();
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].get("size").unwrap().as_str().unwrap(), "512M");
        assert_eq!(partitions[1].get("size").unwrap().as_str().unwrap(), "rest");
    }

    /// Migration 15 must strip `path` from all disk entries in `platforms.attributes`.
    ///
    /// Creates a platform with legacy JSON that includes `path` in each disk entry,
    /// runs all migrations, and verifies the `path` field is absent after migration.
    #[tokio::test]
    async fn test_migration_15_strips_disk_paths() {
        let conn = Connection::open(test_database_path!()).await.unwrap();

        // Bootstrap to migration 14
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations (version) VALUES (0)",
        )
        .await
        .unwrap();

        for version in 1..=14 {
            conn.execute_batch(MIGRATIONS[version - 1]).await.unwrap();
            if let Some(hook) = POST_MIGRATION_HOOKS[version - 1] {
                hook(&conn).await.unwrap();
            }
            conn.execute("UPDATE migrations SET version = ?1", [version])
                .await
                .unwrap();
        }

        // Insert a platform with legacy attributes that still carry `path` on each disk
        let legacy_attrs = r#"{
            "disks": [
                {
                    "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
                    "size_gb": 480,
                    "disk_type": "ssd",
                    "label": "ROOT"
                },
                {
                    "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-2",
                    "size_gb": 2000,
                    "disk_type": "hdd",
                    "label": "DATA1"
                }
            ],
            "nics": [],
            "cpus": [],
            "memory_gib": 32
        }"#;

        conn.execute(
            "INSERT INTO platforms (name, attributes) VALUES ('Legacy Platform', ?1)",
            (legacy_attrs,),
        )
        .await
        .unwrap();

        // Verify path is present before migration
        let stored: String = conn
            .query_row(
                "SELECT attributes FROM platforms WHERE name = 'Legacy Platform'",
                [],
                |row| row.get(0),
            )
            .await
            .unwrap();
        assert!(
            stored.contains("\"path\""),
            "path should be present before migration"
        );

        // Run migration 15
        let factory = test_connection_factory!();
        run_migrations(&factory).await.unwrap();

        // Read via the original connection (shared in-memory DB)
        let stored: String = conn
            .query_row(
                "SELECT attributes FROM platforms WHERE name = 'Legacy Platform'",
                [],
                |row| row.get(0),
            )
            .await
            .unwrap();

        let attrs: serde_json::Value = serde_json::from_str(&stored).unwrap();
        let disks = attrs["disks"].as_array().unwrap();
        assert_eq!(disks.len(), 2, "Both disks should be preserved");
        assert!(
            disks[0].get("path").is_none(),
            "path should be removed from first disk after migration"
        );
        assert!(
            disks[1].get("path").is_none(),
            "path should be removed from second disk after migration"
        );

        // Other fields must be preserved
        assert_eq!(disks[0]["size_gb"], serde_json::json!(480));
        assert_eq!(disks[0]["disk_type"], serde_json::json!("ssd"));
        assert_eq!(disks[0]["label"], serde_json::json!("ROOT"));
        assert_eq!(disks[1]["size_gb"], serde_json::json!(2000));
        assert_eq!(disks[1]["label"], serde_json::json!("DATA1"));
    }
}
