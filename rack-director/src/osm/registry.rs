use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use super::store::{self, OsmModule};
use crate::database::Connection;
use crate::storage::ImageStore;
use osm::manifest::Manifest;
use osm::os_config::OperatingSystemConfig;

/// Loaded OSM data for the bundled Default module read from disk.
#[derive(Debug)]
pub struct BundledOsm {
    pub manifest: Manifest,
    pub os_configs: HashMap<String, OperatingSystemConfig>,
}

/// Read the bundled Default OSM from a directory on disk.
///
/// Expects the directory to contain:
/// - `manifest.toml` at the root
/// - Subdirectories listed in manifest.operating_systems, each with `OperatingSystem.toml`
///
/// Returns None if the directory does not exist or manifest.toml is absent (no bundled OSM
/// shipped).
pub fn load_bundled_osm(path: &Path) -> Result<Option<BundledOsm>> {
    let manifest_path = path.join("manifest.toml");
    if !manifest_path.exists() {
        log::info!(
            "No bundled OSM found at {} (manifest.toml missing)",
            path.display()
        );
        return Ok(None);
    }

    let manifest = load_manifest(&manifest_path)?;
    let os_configs = load_os_configs(path, &manifest.operating_systems)?;

    Ok(Some(BundledOsm {
        manifest,
        os_configs,
    }))
}

/// Parse manifest.toml from disk.
fn load_manifest(manifest_path: &Path) -> Result<Manifest> {
    let manifest_str = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    toml::from_str(&manifest_str)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))
}

/// Load OperatingSystem.toml for each OS directory listed in the manifest.
fn load_os_configs(
    base_path: &Path,
    os_dir_names: &[String],
) -> Result<HashMap<String, OperatingSystemConfig>> {
    let mut os_configs = HashMap::new();
    for os_dir_name in os_dir_names {
        let os_toml_path = base_path.join(os_dir_name).join("OperatingSystem.toml");
        let os_str = std::fs::read_to_string(&os_toml_path)
            .with_context(|| format!("Failed to read {}", os_toml_path.display()))?;
        let config: OperatingSystemConfig = toml::from_str(&os_str)
            .with_context(|| format!("Failed to parse {}", os_toml_path.display()))?;
        os_configs.insert(os_dir_name.clone(), config);
    }
    Ok(os_configs)
}

/// Synchronize the Default OSM in the database with the bundled version.
///
/// Version resolution rules:
/// - If no Default module exists in DB: insert the bundled version
/// - If a Default module exists with source "bundled": always update to bundled version
/// - If a Default module exists with source "uploaded": only update if bundled version is greater
///
/// When updating, the disabled state of OS entries is preserved across the update.
///
/// Returns the active Default module record (whether bundled or uploaded).
pub async fn sync_default_osm(conn: &Connection, bundled: &BundledOsm) -> Result<OsmModule> {
    match store::get_module_by_name(conn, &bundled.manifest.name).await {
        Ok(module) => sync_existing_module(conn, bundled, module).await,
        Err(e) if is_not_found(&e) => insert_fresh_module(conn, bundled).await,
        Err(e) => Err(e),
    }
}

/// Returns `true` only when `err` was caused by a "no rows returned" query result,
/// distinguishing a genuine missing module from a real database failure.
fn is_not_found(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::QueryReturnedNoRows)
        )
    })
}

/// Handle sync when a Default module already exists in the database.
async fn sync_existing_module(
    conn: &Connection,
    bundled: &BundledOsm,
    module: OsmModule,
) -> Result<OsmModule> {
    let existing_version: semver::Version = module.version.parse().with_context(|| {
        format!(
            "Invalid version '{}' in DB for module '{}'",
            module.version, module.name
        )
    })?;

    let should_update =
        determine_update_policy(&module.source, &bundled.manifest.version, &existing_version);

    if should_update {
        apply_module_update(conn, bundled, &module, &existing_version).await
    } else {
        log::info!(
            "Keeping uploaded Default OSM v{} (bundled v{} is not greater)",
            existing_version,
            bundled.manifest.version
        );
        Ok(module)
    }
}

/// Determine whether the bundled version should replace the existing DB record.
fn determine_update_policy(
    source: &str,
    bundled_version: &semver::Version,
    existing_version: &semver::Version,
) -> bool {
    match source {
        // Always update bundled → bundled (rack-director upgrade path)
        "bundled" => true,
        // Only update if bundled version is strictly greater than the uploaded version
        "uploaded" => bundled_version > existing_version,
        _ => false,
    }
}

/// Apply a module update: update the module record and replace its OS entries, preserving
/// disabled state for any OS dirs that exist in both the old and new versions.
async fn apply_module_update(
    conn: &Connection,
    bundled: &BundledOsm,
    module: &OsmModule,
    existing_version: &semver::Version,
) -> Result<OsmModule> {
    let storage_prefix = build_storage_prefix(&bundled.manifest.name, &bundled.manifest.version);

    store::update_module(
        conn,
        module.id,
        &bundled.manifest.version.to_string(),
        &bundled.manifest.author,
        &bundled.manifest.description,
        &storage_prefix,
        None,
    )
    .await?;

    // Restore source to "bundled" — the bundled version now owns this module record.
    store::update_module_source(conn, module.id, "bundled").await?;

    replace_os_entries(conn, module.id, &bundled.os_configs).await?;

    log::info!(
        "Updated Default OSM from {} to {} (source: bundled)",
        existing_version,
        bundled.manifest.version
    );

    store::get_module(conn, module.id).await
}

/// Replace all OS entries for a module, preserving the disabled flag for any dir_name that was
/// disabled in the previous version.
async fn replace_os_entries(
    conn: &Connection,
    module_id: i64,
    os_configs: &HashMap<String, OperatingSystemConfig>,
) -> Result<()> {
    let disabled_dirs = collect_disabled_dirs(conn, module_id).await?;

    store::delete_operating_systems_for_module(conn, module_id).await?;

    for (dir_name, config) in os_configs {
        let os = store::create_operating_system(
            conn,
            module_id,
            dir_name,
            &config.name,
            &config.release,
            config,
        )
        .await?;

        if disabled_dirs.contains(dir_name) {
            store::set_os_disabled(conn, os.id, true).await?;
        }
    }

    Ok(())
}

/// Collect the set of dir_names that are currently disabled for a module.
async fn collect_disabled_dirs(
    conn: &Connection,
    module_id: i64,
) -> Result<std::collections::HashSet<String>> {
    let os_list = store::list_operating_systems(conn, module_id).await?;
    Ok(os_list
        .into_iter()
        .filter(|os| os.disabled)
        .map(|os| os.dir_name)
        .collect())
}

/// Insert a fresh module with all its OS entries (no prior DB record exists).
async fn insert_fresh_module(conn: &Connection, bundled: &BundledOsm) -> Result<OsmModule> {
    let storage_prefix = build_storage_prefix(&bundled.manifest.name, &bundled.manifest.version);

    let module = store::create_module(
        conn,
        &bundled.manifest.name,
        &bundled.manifest.version.to_string(),
        &bundled.manifest.author,
        &bundled.manifest.description,
        "bundled",
        &storage_prefix,
        true,
        None,
    )
    .await?;

    for (dir_name, config) in &bundled.os_configs {
        store::create_operating_system(
            conn,
            module.id,
            dir_name,
            &config.name,
            &config.release,
            config,
        )
        .await?;
    }

    log::info!(
        "Loaded bundled Default OSM v{} ({} operating systems)",
        bundled.manifest.version,
        bundled.os_configs.len()
    );

    Ok(module)
}

/// Build the storage prefix path for an OSM module.
fn build_storage_prefix(name: &str, version: &semver::Version) -> String {
    format!("osm/{name}/{version}/")
}

/// Delete any files under `"osm/"` in the image store that do not belong to a
/// known OSM module.
///
/// When rack-director restarts, in-process grace-period cleanup tasks from the
/// previous run are lost, leaving stale files from superseded module versions.
/// This function reconciles the image store against the database at startup,
/// removing any path under `"osm/"` whose prefix does not match a module's
/// `storage_prefix`.
///
/// Orphaned files are deleted immediately; a count is logged at `info` level.
pub async fn cleanup_orphaned_storage(conn: &Connection, image_store: &ImageStore) -> Result<()> {
    let known_prefixes = collect_known_prefixes(conn).await?;
    let listed_paths = image_store
        .list("osm/")
        .await
        .context("failed to list files under 'osm/'")?;

    let mut deleted = 0usize;
    for path in &listed_paths {
        if !known_prefixes.iter().any(|p| path.starts_with(p.as_str())) {
            image_store
                .delete(path)
                .await
                .with_context(|| format!("failed to delete orphaned file '{path}'"))?;
            deleted += 1;
        }
    }

    log::info!("Startup storage cleanup: deleted {deleted} orphaned OSM file(s)");
    Ok(())
}

/// Collect all `storage_prefix` values from the known OSM modules in the database.
async fn collect_known_prefixes(conn: &Connection) -> Result<Vec<String>> {
    let modules = store::list_modules(conn)
        .await
        .context("failed to list OSM modules")?;
    Ok(modules.into_iter().map(|m| m.storage_prefix).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_connection_factory;
    use std::fs;
    use tempfile::tempdir;

    /// Write a minimal valid bundled OSM layout to a temp directory.
    ///
    /// Creates manifest.toml (v1.0.0, name "Default") and ubuntu/OperatingSystem.toml with one
    /// x86-64 architecture entry.
    fn write_bundled_osm(dir: &Path) {
        let manifest = r#"
name = "Default"
version = "1.0.0"
author = "RD Project"
description = "Default module"
operating_systems = ["ubuntu"]
"#;
        fs::write(dir.join("manifest.toml"), manifest).unwrap();

        let ubuntu_dir = dir.join("ubuntu");
        fs::create_dir_all(ubuntu_dir.join("x86-64")).unwrap();

        let os_config = r#"
name = "Ubuntu"
release = "22.04"
[[architectures]]
arch = "x86-64"
kernel = "x86-64/vmlinuz"
initramfs = "x86-64/initrd.img"
cmdline = "quiet"
install_template = "x86-64/autoinstall.yaml.hbs"
"#;
        fs::write(ubuntu_dir.join("OperatingSystem.toml"), os_config).unwrap();
        fs::write(ubuntu_dir.join("x86-64/vmlinuz"), b"kernel").unwrap();
        fs::write(ubuntu_dir.join("x86-64/initrd.img"), b"initrd").unwrap();
        fs::write(
            ubuntu_dir.join("x86-64/autoinstall.yaml.hbs"),
            b"{{ device.hostname }}",
        )
        .unwrap();
    }

    #[test]
    fn test_load_bundled_osm_success() {
        let dir = tempdir().unwrap();
        write_bundled_osm(dir.path());

        let result = load_bundled_osm(dir.path()).unwrap();
        assert!(result.is_some());
        let bundled = result.unwrap();
        assert_eq!(bundled.manifest.name, "Default");
        assert_eq!(bundled.manifest.version, semver::Version::new(1, 0, 0));
        assert_eq!(bundled.os_configs.len(), 1);
        assert!(bundled.os_configs.contains_key("ubuntu"));
    }

    #[test]
    fn test_load_bundled_osm_missing_dir() {
        let dir = tempdir().unwrap();
        let nonexistent = dir.path().join("no-such-dir");

        let result = load_bundled_osm(&nonexistent).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_bundled_osm_no_manifest() {
        let dir = tempdir().unwrap();
        // Empty directory, no manifest.toml
        let result = load_bundled_osm(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_sync_default_osm_fresh_insert() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        let dir = tempdir().unwrap();
        write_bundled_osm(dir.path());

        let bundled = load_bundled_osm(dir.path()).unwrap().unwrap();
        let module = sync_default_osm(&conn, &bundled).await.unwrap();

        assert_eq!(module.name, "Default");
        assert_eq!(module.version, "1.0.0");
        assert_eq!(module.source, "bundled");
        assert!(
            module.is_default,
            "fresh insert of Default OSM must set is_default = true"
        );

        let os_list = store::list_operating_systems(&conn, module.id)
            .await
            .unwrap();
        assert_eq!(os_list.len(), 1);
        assert_eq!(os_list[0].name, "Ubuntu");
    }

    #[tokio::test]
    async fn test_sync_default_osm_bundled_upgrade() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        // Insert v1.0.0 first
        let dir1 = tempdir().unwrap();
        write_bundled_osm(dir1.path());
        let bundled1 = load_bundled_osm(dir1.path()).unwrap().unwrap();
        sync_default_osm(&conn, &bundled1).await.unwrap();

        // "Upgrade" to v2.0.0 by reusing the same dir with a new manifest
        let dir2 = tempdir().unwrap();
        write_bundled_osm(dir2.path());
        let manifest_v2 = r#"
name = "Default"
version = "2.0.0"
author = "RD Project"
description = "Updated default module"
operating_systems = ["ubuntu"]
"#;
        std::fs::write(dir2.path().join("manifest.toml"), manifest_v2).unwrap();

        let bundled2 = load_bundled_osm(dir2.path()).unwrap().unwrap();
        let module = sync_default_osm(&conn, &bundled2).await.unwrap();

        assert_eq!(module.version, "2.0.0");
    }

    #[tokio::test]
    async fn test_sync_default_osm_uploaded_version_wins() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        // Simulate an uploaded Default v3.0.0
        store::create_module(
            &conn,
            "Default",
            "3.0.0",
            "User",
            "User uploaded",
            "uploaded",
            "osm/Default/3.0.0/",
            false,
            Some("archives/Default-3.0.0.tar.zst"),
        )
        .await
        .unwrap();

        // Bundled is v1.0.0 — should NOT replace uploaded v3.0.0
        let dir = tempdir().unwrap();
        write_bundled_osm(dir.path());
        let bundled = load_bundled_osm(dir.path()).unwrap().unwrap();
        let module = sync_default_osm(&conn, &bundled).await.unwrap();

        assert_eq!(module.version, "3.0.0");
        assert_eq!(module.source, "uploaded");
    }

    #[tokio::test]
    async fn test_sync_default_osm_bundled_beats_old_upload() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        // Simulate an uploaded Default v0.5.0
        store::create_module(
            &conn,
            "Default",
            "0.5.0",
            "User",
            "Old upload",
            "uploaded",
            "osm/Default/0.5.0/",
            false,
            Some("archives/Default-0.5.0.tar.zst"),
        )
        .await
        .unwrap();

        // Bundled is v1.0.0 — should replace uploaded v0.5.0
        let dir = tempdir().unwrap();
        write_bundled_osm(dir.path());
        let bundled = load_bundled_osm(dir.path()).unwrap().unwrap();
        let module = sync_default_osm(&conn, &bundled).await.unwrap();

        assert_eq!(module.version, "1.0.0");
        // Startup sync restores source to "bundled" so routing serves from disk.
        assert_eq!(module.source, "bundled");
    }

    #[tokio::test]
    async fn test_sync_preserves_disabled_state() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        // Insert v1.0.0
        let dir = tempdir().unwrap();
        write_bundled_osm(dir.path());
        let bundled = load_bundled_osm(dir.path()).unwrap().unwrap();
        let module = sync_default_osm(&conn, &bundled).await.unwrap();

        // Disable the ubuntu OS
        let os_list = store::list_operating_systems(&conn, module.id)
            .await
            .unwrap();
        store::set_os_disabled(&conn, os_list[0].id, true)
            .await
            .unwrap();

        // "Upgrade" to v2.0.0 with the same ubuntu OS
        let manifest_v2 = r#"
name = "Default"
version = "2.0.0"
author = "RD Project"
description = "Updated"
operating_systems = ["ubuntu"]
"#;
        std::fs::write(dir.path().join("manifest.toml"), manifest_v2).unwrap();
        let bundled2 = load_bundled_osm(dir.path()).unwrap().unwrap();
        sync_default_osm(&conn, &bundled2).await.unwrap();

        // Verify ubuntu is still disabled after upgrade
        let os_list = store::list_operating_systems(&conn, module.id)
            .await
            .unwrap();
        assert_eq!(os_list.len(), 1);
        assert!(
            os_list[0].disabled,
            "Disabled state should be preserved across upgrade"
        );
    }

    // ── cleanup_orphaned_storage ──────────────────────────────────────────────

    /// Helper: seed a module record in the DB and a file in the store under its prefix.
    async fn seed_module_with_files(
        conn: &Connection,
        store: &ImageStore,
        name: &str,
        version: &str,
        filenames: &[&str],
    ) -> String {
        let prefix = format!("osm/{name}/{version}/");
        super::store::create_module(
            conn, name, version, "Author", "Desc", "uploaded", &prefix, false, None,
        )
        .await
        .unwrap();

        for filename in filenames {
            let path = format!("{prefix}{filename}");
            use tokio_util::io::ReaderStream;
            let bytes: &[u8] = b"data";
            let stream = Box::pin(ReaderStream::new(std::io::Cursor::new(bytes)));
            store.upload(&path, stream).await.unwrap();
        }

        prefix
    }

    /// Files under a known module prefix must NOT be deleted.
    #[tokio::test]
    async fn test_cleanup_keeps_known_files() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        let image_store = ImageStore::memory();

        seed_module_with_files(&conn, &image_store, "my-module", "1.0.0", &["vmlinuz"]).await;

        cleanup_orphaned_storage(&conn, &image_store).await.unwrap();

        assert!(
            image_store
                .exists("osm/my-module/1.0.0/vmlinuz")
                .await
                .unwrap()
        );
    }

    /// Files under a prefix not matching any module must be deleted.
    #[tokio::test]
    async fn test_cleanup_removes_orphaned_files() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        let image_store = ImageStore::memory();

        // Upload a file with no corresponding DB record.
        use tokio_util::io::ReaderStream;
        let stream = Box::pin(ReaderStream::new(std::io::Cursor::new(b"stale")));
        image_store
            .upload("osm/old-module/0.9.0/vmlinuz", stream)
            .await
            .unwrap();

        cleanup_orphaned_storage(&conn, &image_store).await.unwrap();

        assert!(
            !image_store
                .exists("osm/old-module/0.9.0/vmlinuz")
                .await
                .unwrap()
        );
    }

    /// Mixed scenario: known and orphaned files coexist — only orphans are deleted.
    #[tokio::test]
    async fn test_cleanup_mixed_known_and_orphaned() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        let image_store = ImageStore::memory();

        // Seed a known module with files.
        seed_module_with_files(&conn, &image_store, "good-module", "2.0.0", &["kernel"]).await;

        // Upload an orphaned file for a superseded version (no DB row).
        use tokio_util::io::ReaderStream;
        let stream = Box::pin(ReaderStream::new(std::io::Cursor::new(b"old")));
        image_store
            .upload("osm/good-module/1.0.0/kernel", stream)
            .await
            .unwrap();

        cleanup_orphaned_storage(&conn, &image_store).await.unwrap();

        assert!(
            image_store
                .exists("osm/good-module/2.0.0/kernel")
                .await
                .unwrap(),
            "current version file must be kept"
        );
        assert!(
            !image_store
                .exists("osm/good-module/1.0.0/kernel")
                .await
                .unwrap(),
            "old version file must be deleted"
        );
    }

    /// When no files exist under "osm/", cleanup must succeed without error.
    #[tokio::test]
    async fn test_cleanup_empty_store_is_ok() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        let image_store = ImageStore::memory();

        cleanup_orphaned_storage(&conn, &image_store).await.unwrap();
    }
}
