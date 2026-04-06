use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::store::{self, OsmModule};
use crate::database::Connection;
use osm::manifest::Manifest;
use osm::os_config::OperatingSystemConfig;

/// Loaded OSM data for the bundled Default module read from disk.
#[derive(Debug)]
pub struct BundledOsm {
    pub manifest: Manifest,
    pub os_configs: HashMap<String, OperatingSystemConfig>,
    /// Root directory on disk where the bundled OSM files live.
    pub path: PathBuf,
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
        path: path.to_path_buf(),
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

/// Resolve a file path within an OSM module.
///
/// Returns the storage path for a file by combining the module's storage prefix with
/// the OS directory and relative file path.  The result can be passed to ImageStore or
/// used to locate a file under the bundled OSM directory.
pub fn resolve_file_path(module: &OsmModule, os_dir: &str, relative_path: &str) -> String {
    format!("{}{}/{}", module.storage_prefix, os_dir, relative_path)
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
        // source remains "uploaded" because we updated the existing row —
        // name and source are immutable on the record, only version/files change
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

    #[test]
    fn test_resolve_file_path() {
        let module = OsmModule {
            id: 1,
            name: "Default".to_string(),
            version: "1.0.0".to_string(),
            author: "Test".to_string(),
            description: "Test".to_string(),
            source: "bundled".to_string(),
            storage_prefix: "osm/Default/1.0.0/".to_string(),
            archive_path: None,
            created_at: None,
            updated_at: None,
        };

        let path = resolve_file_path(&module, "ubuntu", "x86-64/vmlinuz");
        assert_eq!(path, "osm/Default/1.0.0/ubuntu/x86-64/vmlinuz");
    }
}
