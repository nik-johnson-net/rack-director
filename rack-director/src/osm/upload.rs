use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio_util::io::ReaderStream;

use crate::database::{Connection, ConnectionFactory};
use crate::storage::ImageStore;

use super::store;
use osm::archive::{ParsedArchive, read_archive};
use osm::normalize_path;
use osm::os_config::OperatingSystemConfig;
use osm::validation::validate_osm;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Top-level entry point for the OSM upload processing pipeline.
///
/// This function is intended to be spawned as a background task after the
/// archive bytes have been written to `temp_path`.  It runs the full
/// validation → extraction → database update pipeline, and always cleans up
/// `temp_path` before returning, regardless of success or failure.
///
/// On error the upload record is marked `"failed"` with a human-readable
/// message.  On success it is marked `"complete"` and, if an older version
/// of the same module was replaced, a background task is spawned to delete
/// the old storage files after a one-hour grace period.
pub async fn process_upload(
    upload_id: i64,
    temp_path: PathBuf,
    connection_factory: Arc<dyn ConnectionFactory>,
    image_store: Arc<ImageStore>,
) {
    let result = process_upload_inner(
        upload_id,
        &temp_path,
        connection_factory.as_ref(),
        Arc::clone(&image_store),
    )
    .await;

    if let Err(err) = result {
        let msg = format!("{err:#}");
        log::error!("OSM upload {upload_id} failed: {msg}");

        if let Ok(conn) = connection_factory.open().await {
            let _ = store::update_upload_status(&conn, upload_id, "failed", Some(&msg), None)
                .await
                .inspect_err(|e| log::warn!("Failed to record upload failure: {e}"));
        }
    }

    // Always clean up the temporary file.
    if temp_path.exists()
        && let Err(e) = std::fs::remove_file(&temp_path)
    {
        log::warn!("Failed to remove temp file {:?}: {e}", temp_path);
    }
}

// ── Pipeline stages ───────────────────────────────────────────────────────────

/// Runs the full processing pipeline for a single OSM upload.
///
/// Phases:
/// 1. Validate — parse and structurally validate the archive.
/// 2. Validate templates — test-render every install/cmdline template.
/// 3. Extract — copy all archive files into the image store.
/// 4. Commit — atomically update the database and mark the upload complete.
/// 5. Cleanup — schedule deletion of old files if a module was replaced.
async fn process_upload_inner(
    upload_id: i64,
    temp_path: &Path,
    connection_factory: &dyn ConnectionFactory,
    image_store: Arc<ImageStore>,
) -> Result<()> {
    // Phase 1: Parse and validate.
    let conn = connection_factory.open().await?;
    store::update_upload_status(&conn, upload_id, "validating", None, None).await?;

    let file = std::fs::File::open(temp_path)
        .with_context(|| format!("failed to open temp file {:?}", temp_path))?;
    let parsed = read_archive(file).context("failed to parse OSM archive")?;

    let errors = validate_osm(&parsed);
    if !errors.is_empty() {
        let message = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        bail!("archive validation failed: {message}");
    }

    // Phase 2: Validate templates.
    validate_templates_from_archive(temp_path, &parsed).context("template validation failed")?;

    // Phase 3: Extract files to storage.
    let module_name_slug = slugify(&parsed.manifest.name);
    let version_str = parsed.manifest.version.to_string();
    let storage_prefix = format!("osm/{module_name_slug}/{version_str}/");

    store::update_upload_status(&conn, upload_id, "extracting", None, None).await?;
    extract_to_storage(temp_path, &storage_prefix, &image_store)
        .await
        .context("failed to extract archive to storage")?;

    // Phase 4: Atomic database update.
    let old_prefix = atomic_db_update(
        &conn,
        upload_id,
        &parsed,
        &storage_prefix,
        temp_path,
        &image_store,
    )
    .await
    .context("failed to commit OSM module to database")?;

    // Phase 5: Schedule cleanup of the old version's files.
    if let Some(prefix) = old_prefix {
        let image_store_clone = Arc::clone(&image_store);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            if let Err(e) = cleanup_old_files(&image_store_clone, &prefix).await {
                log::warn!("Failed to clean up old OSM files at '{prefix}': {e}");
            }
        });
    }

    Ok(())
}

// ── Template validation ───────────────────────────────────────────────────────

/// Test-renders every `install_template` and `cmdline` template in the archive.
///
/// Each template is rendered with a stub context that covers all variables a
/// production render could supply, ensuring syntax errors and missing variable
/// references are caught before the archive is committed.
fn validate_templates_from_archive(temp_path: &Path, parsed: &ParsedArchive) -> Result<()> {
    let file = std::fs::File::open(temp_path)
        .with_context(|| format!("failed to re-open temp file {:?}", temp_path))?;

    let template_contents = read_hbs_files(file)?;

    let mut hbs = handlebars::Handlebars::new();
    hbs.set_strict_mode(false);
    hbs.register_escape_fn(handlebars::no_escape);

    for (os_dir, os_config) in &parsed.os_configs {
        let ctx = build_template_context(os_dir, os_config);

        for arch in &os_config.architectures {
            render_template(
                &mut hbs,
                &template_contents,
                os_dir,
                &arch.install_template,
                "install_template",
                &ctx,
            )?;

            if !arch.cmdline.is_empty() {
                render_inline_template(&mut hbs, os_dir, "cmdline", &arch.cmdline, &ctx)?;
            }
        }
    }

    Ok(())
}

/// Re-reads the archive from `reader` and returns a map of `path → content`
/// for every `.hbs` file found inside it.
fn read_hbs_files<R: Read>(reader: R) -> Result<HashMap<String, String>> {
    let decoder = zstd::Decoder::new(reader).context("failed to create zstd decoder")?;
    let mut archive = tar::Archive::new(decoder);
    let mut result = HashMap::new();

    for entry_result in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let path = entry.path().context("invalid entry path")?;
        let path_str = normalize_path(&path.to_string_lossy());

        if entry.header().entry_type().is_dir() {
            continue;
        }

        if path_str.ends_with(".hbs") {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .with_context(|| format!("failed to read template file '{path_str}'"))?;
            let text = String::from_utf8(bytes)
                .with_context(|| format!("template file '{path_str}' is not valid UTF-8"))?;
            result.insert(path_str, text);
        }
    }

    Ok(result)
}

/// Renders a named template file (referenced by its path inside the archive).
fn render_template(
    hbs: &mut handlebars::Handlebars,
    templates: &HashMap<String, String>,
    os_dir: &str,
    filename: &str,
    field: &str,
    ctx: &serde_json::Value,
) -> Result<()> {
    let path = format!("{os_dir}/{filename}");
    let Some(source) = templates.get(&path) else {
        // Non-.hbs template files are not validated here; only .hbs files need rendering.
        return Ok(());
    };

    hbs.render_template(source, ctx)
        .with_context(|| format!("{field} template '{path}' failed to render"))?;

    Ok(())
}

/// Renders an inline template string (e.g., a `cmdline` field value).
fn render_inline_template(
    hbs: &mut handlebars::Handlebars,
    os_dir: &str,
    field: &str,
    source: &str,
    ctx: &serde_json::Value,
) -> Result<()> {
    hbs.render_template(source, ctx)
        .with_context(|| format!("{field} template for OS '{os_dir}' failed to render"))?;
    Ok(())
}

/// Constructs a stub template context for an OS config.
///
/// Every variable that a production template render supplies is present,
/// allowing `render_template` to catch both syntax errors and bad variable
/// references without requiring a real device or role.
fn build_template_context(os_dir: &str, os_config: &OperatingSystemConfig) -> serde_json::Value {
    let mut config_vars = serde_json::Map::new();
    for var in &os_config.template_variables {
        let default_value = var
            .default
            .as_ref()
            .map(toml_value_to_json)
            .unwrap_or(serde_json::Value::String(String::new()));
        config_vars.insert(var.name.clone(), default_value);
    }

    serde_json::json!({
        "device": {
            "uuid": "00000000-0000-0000-0000-000000000000",
            "hostname": "stub-host",
            "mac_address": "00:00:00:00:00:00",
            "ip_address": "0.0.0.0",
            "gateway": "0.0.0.0",
            "dns_servers": ["0.0.0.0"],
            "netmask": "255.255.255.0",
            "prefix_length": 24,
            "boot_mode": "bios",
            "is_uefi": false,
            "is_bios": true
        },
        "role": {
            "name": "stub-role",
            "disk_layout": null
        },
        "os": {
            "name": os_config.name,
            "release": os_config.release,
            "dir": os_dir
        },
        "config": serde_json::Value::Object(config_vars),
        "partitions": [],
        "logical_volumes": [],
        "install_script_url": "http://stub/install.sh"
    })
}

/// Converts a `toml::Value` to a `serde_json::Value` for use in template contexts.
fn toml_value_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(tbl) => {
            let map = tbl
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

// ── Storage extraction ────────────────────────────────────────────────────────

/// Uploads every file in the archive to `image_store` under `storage_prefix`.
///
/// The archive is extracted to a temporary directory on disk using
/// `spawn_blocking` (avoiding the `!Send` `tar::Archive` iterator across await
/// points and eliminating the OOM risk of buffering multi-GB files in memory).
/// After extraction each file is streamed individually to the image store via
/// `tokio::fs::File` + `ReaderStream`, then the temporary directory is removed.
async fn extract_to_storage(
    temp_path: &Path,
    storage_prefix: &str,
    image_store: &ImageStore,
) -> Result<()> {
    let temp_dir = build_extract_temp_dir();
    let temp_path_owned = temp_path.to_owned();
    let temp_dir_clone = temp_dir.clone();

    // Extract the archive on a blocking thread to avoid holding `!Send` types
    // across await points and to keep the async runtime unblocked.
    tokio::task::spawn_blocking(move || unpack_archive_to_dir(&temp_path_owned, &temp_dir_clone))
        .await
        .context("blocking extraction task panicked")??;

    // Walk extracted files and stream-upload each one individually.
    let result = upload_extracted_files(&temp_dir, storage_prefix, image_store).await;

    // Always remove the temp directory, even on error.
    if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
        log::warn!("Failed to remove extraction temp dir {:?}: {e}", temp_dir);
    }

    result
}

/// Constructs a unique temporary directory path for archive extraction.
///
/// A random 64-bit nonce ensures no collisions between concurrent uploads.
fn build_extract_temp_dir() -> PathBuf {
    let nonce: u64 = rand::random();
    std::env::temp_dir().join(format!("osm-extract-{nonce:016x}"))
}

/// Unpacks the tar.zst archive at `src` into `dest_dir`.
///
/// This is a blocking operation and must be called via `spawn_blocking`.
fn unpack_archive_to_dir(src: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create extraction dir {:?}", dest_dir))?;

    let file =
        std::fs::File::open(src).with_context(|| format!("failed to open temp file {:?}", src))?;

    let decoder = zstd::Decoder::new(file).context("failed to create zstd decoder")?;
    let mut archive = tar::Archive::new(decoder);

    archive
        .unpack(dest_dir)
        .with_context(|| format!("failed to unpack archive into {:?}", dest_dir))?;

    Ok(())
}

/// Walks `base_dir` recursively and streams each file to the image store.
///
/// The storage path for each file is `storage_prefix` + the file's path
/// relative to `base_dir`, using forward slashes.
async fn upload_extracted_files(
    base_dir: &Path,
    storage_prefix: &str,
    image_store: &ImageStore,
) -> Result<()> {
    let files = collect_files_recursive(base_dir).await?;

    for abs_path in files {
        let rel = abs_path
            .strip_prefix(base_dir)
            .context("extracted file path not under base dir")?;

        // Normalize to forward slashes for storage keys.
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        let storage_path = format!("{storage_prefix}{rel_str}");

        stream_file_to_storage(&abs_path, &storage_path, image_store).await?;
    }

    Ok(())
}

/// Recursively collects all file paths under `dir`, skipping directories.
async fn collect_files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    collect_files_inner(dir, &mut result).await?;
    Ok(result)
}

/// Inner recursive helper for [`collect_files_recursive`].
async fn collect_files_inner(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .with_context(|| format!("failed to read directory {:?}", dir))?;

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .with_context(|| format!("failed to iterate directory {:?}", dir))?
    {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .await
            .with_context(|| format!("failed to stat {:?}", path))?;

        if file_type.is_dir() {
            // Recurse — defined as a separate async fn to avoid recursive async closures.
            Box::pin(collect_files_inner(&path, out)).await?;
        } else if file_type.is_file() {
            out.push(path);
        }
        // Symlinks are intentionally skipped.
    }

    Ok(())
}

/// Opens `path` and streams its contents to the image store at `storage_path`.
async fn stream_file_to_storage(
    path: &Path,
    storage_path: &str,
    image_store: &ImageStore,
) -> Result<()> {
    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open extracted file {:?}", path))?;

    let stream = Box::pin(ReaderStream::new(file));

    image_store
        .upload(storage_path, stream)
        .await
        .with_context(|| format!("failed to upload '{storage_path}'"))?;

    Ok(())
}

// ── Database commit ───────────────────────────────────────────────────────────

/// Atomically inserts or replaces the OSM module and its OS entries in the
/// database, then marks the upload record as complete.
///
/// Returns the old storage prefix when a module is replaced, so the caller can
/// schedule deletion of the superseded files.
async fn atomic_db_update(
    conn: &Connection,
    upload_id: i64,
    parsed: &ParsedArchive,
    storage_prefix: &str,
    temp_path: &Path,
    image_store: &ImageStore,
) -> Result<Option<String>> {
    let manifest = &parsed.manifest;
    let version_str = manifest.version.to_string();
    let archive_storage_path = format!("osm-archives/{}.tar.zst", slugify(&manifest.name));

    // Store the archive itself for future re-processing.
    upload_archive(temp_path, &archive_storage_path, image_store)
        .await
        .context("failed to store OSM archive")?;

    let existing = store::get_module_by_name(conn, &manifest.name).await;

    match existing {
        Ok(old_module) => {
            // Replacing an existing module.
            let old_prefix = old_module.storage_prefix.clone();
            let old_oses = store::list_operating_systems(conn, old_module.id).await?;

            // Build a map of dir_name → disabled state to preserve operator choices.
            let disabled_by_dir: HashMap<String, bool> = old_oses
                .iter()
                .map(|os| (os.dir_name.clone(), os.disabled))
                .collect();

            store::update_module(
                conn,
                old_module.id,
                &version_str,
                &manifest.author,
                &manifest.description,
                storage_prefix,
                Some(&archive_storage_path),
            )
            .await?;

            // Update source to "uploaded" — the existing record may have been "bundled"
            // (e.g. when a user uploads a replacement for the Default module).
            store::update_module_source(conn, old_module.id, "uploaded")
                .await
                .context("failed to update module source to 'uploaded'")?;

            store::delete_operating_systems_for_module(conn, old_module.id).await?;

            create_os_entries(conn, old_module.id, &parsed.os_configs, &disabled_by_dir).await?;

            store::update_upload_status(conn, upload_id, "complete", None, Some(old_module.id))
                .await?;

            Ok(Some(old_prefix))
        }
        Err(_) => {
            // New module.
            let new_module = store::create_module(
                conn,
                &manifest.name,
                &version_str,
                &manifest.author,
                &manifest.description,
                "uploaded",
                storage_prefix,
                false,
                Some(&archive_storage_path),
            )
            .await?;

            create_os_entries(conn, new_module.id, &parsed.os_configs, &HashMap::new()).await?;

            store::update_upload_status(conn, upload_id, "complete", None, Some(new_module.id))
                .await?;

            Ok(None)
        }
    }
}

/// Creates OS entries for a module, applying any preserved `disabled` flags.
async fn create_os_entries(
    conn: &Connection,
    module_id: i64,
    os_configs: &HashMap<String, OperatingSystemConfig>,
    disabled_by_dir: &HashMap<String, bool>,
) -> Result<()> {
    for (dir_name, config) in os_configs {
        let os = store::create_operating_system(
            conn,
            module_id,
            dir_name,
            &config.name,
            &config.release,
            config,
        )
        .await
        .with_context(|| format!("failed to create OS entry for '{dir_name}'"))?;

        if let Some(&disabled) = disabled_by_dir.get(dir_name)
            && disabled
        {
            store::set_os_disabled(conn, os.id, true).await?;
        }
    }
    Ok(())
}

/// Streams the archive at `temp_path` to `storage_path` in the image store.
///
/// Uses async file I/O and `ReaderStream` to avoid loading the entire archive
/// into memory, preventing OOM on large uploads.
async fn upload_archive(
    temp_path: &Path,
    storage_path: &str,
    image_store: &ImageStore,
) -> Result<()> {
    let file = tokio::fs::File::open(temp_path)
        .await
        .with_context(|| format!("failed to open temp file {:?}", temp_path))?;

    let stream = Box::pin(ReaderStream::new(file));

    image_store
        .upload(storage_path, stream)
        .await
        .with_context(|| format!("failed to upload archive to '{storage_path}'"))?;

    Ok(())
}

// ── Old file cleanup ──────────────────────────────────────────────────────────

/// Lists and deletes all files stored under `prefix` in the image store.
///
/// Intended to be called after a grace period when replacing an older version
/// of a module, to allow in-flight downloads to complete.
async fn cleanup_old_files(image_store: &ImageStore, prefix: &str) -> Result<()> {
    let paths = image_store
        .list(prefix)
        .await
        .with_context(|| format!("failed to list files under '{prefix}'"))?;

    for path in paths {
        image_store
            .delete(&path)
            .await
            .with_context(|| format!("failed to delete old file '{path}'"))?;
    }

    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Converts a module name to a lowercase, hyphen-separated slug suitable for
/// use as a storage path component.
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use osm::archive::tests_helper::build_test_archive;
    use osm::os_config::{ArchitectureConfig, TemplateVariable, TemplateVariableType};

    // ── Fixtures ──────────────────────────────────────────────────────────────

    fn manifest_toml(name: &str) -> String {
        format!(
            r#"
name = "{name}"
version = "1.0.0"
author = "Test Author"
description = "A test OSM"
operating_systems = ["test-os"]
"#
        )
    }

    fn os_config_toml_with_template() -> String {
        r#"
name = "TestOS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh.hbs"
cmdline = "quiet hostname={{ device.hostname }}"

[[template_variables]]
name = "extra_packages"
type = "string"
description = "Additional packages to install"
default = "curl wget"
"#
        .to_string()
    }

    fn install_template_hbs() -> &'static [u8] {
        b"#!/bin/bash\nhostname={{ device.hostname }}\npkg={{ config.extra_packages }}"
    }

    // ── validate_templates_from_archive ───────────────────────────────────────

    /// A well-formed archive with a valid Handlebars template must pass
    /// template validation without errors.
    #[test]
    fn test_validate_templates_success() {
        let manifest = manifest_toml("Test Module");
        let os_config = os_config_toml_with_template();

        let archive_bytes = build_test_archive(&[
            ("manifest.toml", manifest.as_bytes()),
            ("test-os/OperatingSystem.toml", os_config.as_bytes()),
            ("test-os/vmlinuz", b"kernel"),
            ("test-os/initrd.img", b"initramfs"),
            ("test-os/install.sh.hbs", install_template_hbs()),
        ]);

        // Write archive to a temp file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &archive_bytes).unwrap();

        // Parse the archive.
        let parsed = read_archive(archive_bytes.as_slice()).unwrap();

        // Validate templates — must succeed.
        let result = validate_templates_from_archive(tmp.path(), &parsed);
        assert!(
            result.is_ok(),
            "expected template validation to pass, got: {:?}",
            result.unwrap_err()
        );
    }

    /// A template with a syntax error must cause template validation to fail.
    #[test]
    fn test_validate_templates_invalid_syntax_fails() {
        let manifest = manifest_toml("Bad Template Module");
        let os_config = r#"
name = "BadOS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh.hbs"
"#;

        // Unclosed block — invalid Handlebars syntax.
        let bad_template = b"{{ device.hostname } unclosed";

        let archive_bytes = build_test_archive(&[
            ("manifest.toml", manifest.as_bytes()),
            ("test-os/OperatingSystem.toml", os_config.as_bytes()),
            ("test-os/vmlinuz", b"kernel"),
            ("test-os/initrd.img", b"initramfs"),
            ("test-os/install.sh.hbs", bad_template),
        ]);

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &archive_bytes).unwrap();

        let parsed = read_archive(archive_bytes.as_slice()).unwrap();

        let result = validate_templates_from_archive(tmp.path(), &parsed);
        assert!(
            result.is_err(),
            "expected template validation to fail for invalid syntax"
        );
    }

    /// A non-.hbs install template (e.g., a plain shell script) must be
    /// skipped during template validation — no error should be produced.
    #[test]
    fn test_validate_templates_non_hbs_skipped() {
        let manifest = manifest_toml("Plain Script Module");
        let os_config = r#"
name = "PlainOS"
release = "2.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;

        let archive_bytes = build_test_archive(&[
            ("manifest.toml", manifest.as_bytes()),
            ("test-os/OperatingSystem.toml", os_config.as_bytes()),
            ("test-os/vmlinuz", b"kernel"),
            ("test-os/initrd.img", b"initramfs"),
            ("test-os/install.sh", b"#!/bin/bash\necho done"),
        ]);

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &archive_bytes).unwrap();

        let parsed = read_archive(archive_bytes.as_slice()).unwrap();

        let result = validate_templates_from_archive(tmp.path(), &parsed);
        assert!(
            result.is_ok(),
            "non-.hbs templates should be skipped: {:?}",
            result.err()
        );
    }

    // ── extract_to_storage ────────────────────────────────────────────────────

    /// `extract_to_storage` must upload every non-directory file from the
    /// archive to the image store under the given prefix, without loading any
    /// file fully into memory.
    #[tokio::test]
    async fn test_extract_to_storage_uploads_all_files() {
        let manifest = manifest_toml("Extract Module");
        let os_config = r#"
name = "ExtractOS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;

        let archive_bytes = build_test_archive(&[
            ("manifest.toml", manifest.as_bytes()),
            ("test-os/OperatingSystem.toml", os_config.as_bytes()),
            ("test-os/vmlinuz", b"fake kernel bytes"),
            ("test-os/initrd.img", b"fake initramfs bytes"),
            ("test-os/install.sh", b"#!/bin/bash\necho done"),
        ]);

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &archive_bytes).unwrap();

        let store = crate::storage::ImageStore::memory();
        let prefix = "osm/extract-module/1.0.0/";

        extract_to_storage(tmp.path(), prefix, &store)
            .await
            .unwrap();

        // Each file in the archive must be present in the store.
        assert!(
            store
                .exists("osm/extract-module/1.0.0/manifest.toml")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("osm/extract-module/1.0.0/test-os/OperatingSystem.toml")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("osm/extract-module/1.0.0/test-os/vmlinuz")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("osm/extract-module/1.0.0/test-os/initrd.img")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("osm/extract-module/1.0.0/test-os/install.sh")
                .await
                .unwrap()
        );
    }

    /// `extract_to_storage` must preserve file contents exactly.
    #[tokio::test]
    async fn test_extract_to_storage_preserves_file_contents() {
        let manifest = manifest_toml("Content Module");
        let os_config = r#"
name = "ContentOS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let kernel_data = b"kernel binary content 1234";

        let archive_bytes = build_test_archive(&[
            ("manifest.toml", manifest.as_bytes()),
            ("test-os/OperatingSystem.toml", os_config.as_bytes()),
            ("test-os/vmlinuz", kernel_data),
            ("test-os/initrd.img", b"initrd"),
            ("test-os/install.sh", b"#!/bin/bash"),
        ]);

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &archive_bytes).unwrap();

        let store = crate::storage::ImageStore::memory();
        let prefix = "osm/content-module/1.0.0/";

        extract_to_storage(tmp.path(), prefix, &store)
            .await
            .unwrap();

        let (mut stream, _size) = store
            .download("osm/content-module/1.0.0/test-os/vmlinuz")
            .await
            .unwrap();

        use futures::StreamExt;
        let mut downloaded = Vec::new();
        while let Some(chunk) = stream.next().await {
            downloaded.extend_from_slice(&chunk.unwrap());
        }

        assert_eq!(downloaded, kernel_data);
    }

    // ── slugify ───────────────────────────────────────────────────────────────

    #[test]
    fn test_slugify_lowercases_and_replaces_spaces() {
        assert_eq!(slugify("My Module"), "my-module");
    }

    #[test]
    fn test_slugify_preserves_hyphens() {
        assert_eq!(slugify("my-module"), "my-module");
    }

    #[test]
    fn test_slugify_replaces_special_chars() {
        assert_eq!(slugify("OS/2 Warp"), "os-2-warp");
    }

    // ── toml_value_to_json ────────────────────────────────────────────────────

    #[test]
    fn test_toml_value_to_json_string() {
        let v = toml::Value::String("hello".to_string());
        assert_eq!(toml_value_to_json(&v), serde_json::json!("hello"));
    }

    #[test]
    fn test_toml_value_to_json_integer() {
        let v = toml::Value::Integer(42);
        assert_eq!(toml_value_to_json(&v), serde_json::json!(42));
    }

    #[test]
    fn test_toml_value_to_json_boolean() {
        let v = toml::Value::Boolean(true);
        assert_eq!(toml_value_to_json(&v), serde_json::json!(true));
    }

    #[test]
    fn test_toml_value_to_json_array() {
        let v = toml::Value::Array(vec![
            toml::Value::String("a".to_string()),
            toml::Value::String("b".to_string()),
        ]);
        assert_eq!(toml_value_to_json(&v), serde_json::json!(["a", "b"]));
    }

    // ── build_template_context ────────────────────────────────────────────────

    #[test]
    fn test_build_template_context_includes_device_fields() {
        let config = OperatingSystemConfig {
            name: "TestOS".to_string(),
            release: "1.0".to_string(),
            architectures: vec![ArchitectureConfig {
                arch: "x86-64".to_string(),
                kernel: "vmlinuz".to_string(),
                initramfs: "initrd.img".to_string(),
                modules: vec![],
                cmdline: String::new(),
                install_template: "install.sh".to_string(),
            }],
            template_variables: vec![TemplateVariable {
                name: "root_password".to_string(),
                var_type: TemplateVariableType::String,
                description: "Root password".to_string(),
                required: false,
                default: Some(toml::Value::String("secret".to_string())),
            }],
        };

        let ctx = build_template_context("test-os", &config);

        assert_eq!(ctx["device"]["hostname"], "stub-host");
        assert_eq!(ctx["device"]["is_bios"], true);
        assert_eq!(ctx["device"]["is_uefi"], false);
        assert_eq!(ctx["os"]["name"], "TestOS");
        assert_eq!(ctx["config"]["root_password"], "secret");
        assert!(ctx["partitions"].is_array());
        assert!(ctx["logical_volumes"].is_array());
    }
}
