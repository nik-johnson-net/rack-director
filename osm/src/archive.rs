use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, anyhow};

use crate::manifest::Manifest;
use crate::os_config::OperatingSystemConfig;

/// A fully-parsed OSM archive, ready for validation and extraction.
#[derive(Debug)]
pub struct ParsedArchive {
    /// The root-level manifest describing the archive contents.
    pub manifest: Manifest,
    /// Map from OS subdirectory name to its parsed `OperatingSystem.toml`.
    pub os_configs: HashMap<String, OperatingSystemConfig>,
    /// All file paths present in the archive (directories excluded).
    pub file_inventory: Vec<String>,
}

/// Reads a zstd-compressed tar archive from `reader` and parses its contents.
///
/// The archive is expected to contain:
/// - `manifest.toml` at the root
/// - `{os_dir}/OperatingSystem.toml` for each OS subdirectory declared in the manifest
///
/// Paths with a leading `./` are normalized automatically. Directories are skipped.
/// Returns an error if `manifest.toml` is absent or cannot be parsed, or if any
/// `OperatingSystem.toml` cannot be parsed.
pub fn read_archive<R: Read>(reader: R) -> Result<ParsedArchive> {
    let decoder = zstd::Decoder::new(reader).context("failed to create zstd decoder")?;
    let mut archive = tar::Archive::new(decoder);

    let mut manifest_bytes: Option<Vec<u8>> = None;
    // (os_dir, raw toml bytes)
    let mut os_config_bytes: Vec<(String, Vec<u8>)> = Vec::new();
    let mut file_inventory: Vec<String> = Vec::new();

    for entry_result in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let path = entry.path().context("invalid entry path")?;

        let path_str = normalize_path(&path.to_string_lossy());

        // Skip directories
        if entry.header().entry_type().is_dir() {
            continue;
        }

        file_inventory.push(path_str.clone());

        if path_str == "manifest.toml" {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .context("failed to read manifest.toml")?;
            manifest_bytes = Some(bytes);
        } else if let Some(os_dir) = parse_os_config_path(&path_str) {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .context("failed to read OperatingSystem.toml")?;
            os_config_bytes.push((os_dir.to_owned(), bytes));
        }
    }

    let manifest = parse_manifest(manifest_bytes)?;
    let os_configs = parse_os_configs(os_config_bytes)?;

    Ok(ParsedArchive {
        manifest,
        os_configs,
        file_inventory,
    })
}

/// Strips a leading `./` from an archive path component.
fn normalize_path(raw: &str) -> String {
    raw.strip_prefix("./").unwrap_or(raw).to_owned()
}

/// Returns the OS directory name if `path` is exactly `{dir}/OperatingSystem.toml`
/// where `dir` contains no `/` (i.e., is one level deep).
fn parse_os_config_path(path: &str) -> Option<&str> {
    let remainder = path.strip_suffix("/OperatingSystem.toml")?;
    // The remainder must be a single directory component with no slashes.
    if remainder.contains('/') {
        return None;
    }
    Some(remainder)
}

/// Deserializes `manifest.toml` bytes, returning an error if they are absent or invalid.
fn parse_manifest(bytes: Option<Vec<u8>>) -> Result<Manifest> {
    let bytes = bytes.ok_or_else(|| anyhow!("archive is missing manifest.toml"))?;
    let text = std::str::from_utf8(&bytes).context("manifest.toml is not valid UTF-8")?;
    toml::from_str(text).context("failed to parse manifest.toml")
}

/// Deserializes all collected `OperatingSystem.toml` payloads into a map keyed by OS dir.
fn parse_os_configs(raw: Vec<(String, Vec<u8>)>) -> Result<HashMap<String, OperatingSystemConfig>> {
    let mut map = HashMap::new();
    for (os_dir, bytes) in raw {
        let text = std::str::from_utf8(&bytes)
            .context(format!("{os_dir}/OperatingSystem.toml is not valid UTF-8"))?;
        let config: OperatingSystemConfig = toml::from_str(text)
            .context(format!("failed to parse {os_dir}/OperatingSystem.toml"))?;
        map.insert(os_dir, config);
    }
    Ok(map)
}

pub mod tests_helper {
    use std::io::Write;

    /// Builds a zstd-compressed tar archive in memory from the given `(path, content)` pairs.
    ///
    /// Intended for use in unit tests across the `osm` module.
    pub fn build_test_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        // Build uncompressed tar into a buffer first.
        let tar_buf: Vec<u8> = Vec::new();
        let mut tar_builder = tar::Builder::new(tar_buf);

        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar_builder
                .append_data(&mut header, path, *content)
                .expect("failed to append file to test archive");
        }

        let tar_bytes = tar_builder
            .into_inner()
            .expect("failed to finalize test tar");

        // Compress with zstd.
        let mut compressed = Vec::new();
        let mut encoder =
            zstd::Encoder::new(&mut compressed, 1).expect("failed to create zstd encoder");
        encoder
            .write_all(&tar_bytes)
            .expect("failed to write tar to zstd encoder");
        encoder.finish().expect("failed to finalize zstd encoder");

        compressed
    }
}

#[cfg(test)]
mod tests {
    use super::tests_helper::build_test_archive;
    use super::*;

    const MANIFEST_TOML: &[u8] = br#"
name = "Test Module"
version = "1.0.0"
author = "Test Author"
description = "A test OSM"
operating_systems = ["ubuntu-2204"]
"#;

    const OS_CONFIG_TOML: &[u8] = br#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;

    #[test]
    fn test_valid_archive_with_manifest_and_os_config() {
        let archive = build_test_archive(&[
            ("manifest.toml", MANIFEST_TOML),
            ("ubuntu-2204/OperatingSystem.toml", OS_CONFIG_TOML),
            ("ubuntu-2204/vmlinuz", b"kernel-binary"),
            ("ubuntu-2204/initrd.img", b"initramfs-binary"),
            ("ubuntu-2204/install.sh", b"#!/bin/bash\necho install"),
        ]);

        let parsed = read_archive(archive.as_slice()).unwrap();

        assert_eq!(parsed.manifest.name, "Test Module");
        assert_eq!(parsed.manifest.operating_systems, vec!["ubuntu-2204"]);
        assert!(parsed.os_configs.contains_key("ubuntu-2204"));
        let os = &parsed.os_configs["ubuntu-2204"];
        assert_eq!(os.name, "Ubuntu");
        assert_eq!(os.release, "22.04");

        // File inventory should contain all non-directory entries.
        assert!(parsed.file_inventory.contains(&"manifest.toml".to_string()));
        assert!(
            parsed
                .file_inventory
                .contains(&"ubuntu-2204/OperatingSystem.toml".to_string())
        );
        assert!(
            parsed
                .file_inventory
                .contains(&"ubuntu-2204/vmlinuz".to_string())
        );
    }

    #[test]
    fn test_missing_manifest_returns_error() {
        let archive = build_test_archive(&[("ubuntu-2204/OperatingSystem.toml", OS_CONFIG_TOML)]);

        let result = read_archive(archive.as_slice());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("missing manifest.toml"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_invalid_manifest_toml_returns_error() {
        let archive = build_test_archive(&[("manifest.toml", b"not valid toml [[[[")]);

        let result = read_archive(archive.as_slice());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("manifest.toml"), "unexpected error: {msg}");
    }

    #[test]
    fn test_invalid_os_config_toml_returns_error() {
        let archive = build_test_archive(&[
            ("manifest.toml", MANIFEST_TOML),
            ("ubuntu-2204/OperatingSystem.toml", b"not valid toml [[[["),
        ]);

        let result = read_archive(archive.as_slice());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("OperatingSystem.toml"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_dot_slash_prefix_normalization() {
        // Paths prefixed with "./" should be treated identically to those without.
        let archive = build_test_archive(&[
            ("./manifest.toml", MANIFEST_TOML),
            ("./ubuntu-2204/OperatingSystem.toml", OS_CONFIG_TOML),
            ("./ubuntu-2204/vmlinuz", b"kernel"),
        ]);

        let parsed = read_archive(archive.as_slice()).unwrap();
        assert!(parsed.os_configs.contains_key("ubuntu-2204"));
        assert!(parsed.file_inventory.contains(&"manifest.toml".to_string()));
        assert!(
            parsed
                .file_inventory
                .contains(&"ubuntu-2204/vmlinuz".to_string())
        );
    }

    #[test]
    fn test_multiple_os_directories() {
        let rhel_config: &[u8] = br#"
name = "RHEL"
release = "9"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let manifest: &[u8] = br#"
name = "Multi OS"
version = "2.0.0"
author = "Tester"
description = "Two OSes"
operating_systems = ["ubuntu-2204", "rhel-9"]
"#;

        let archive = build_test_archive(&[
            ("manifest.toml", manifest),
            ("ubuntu-2204/OperatingSystem.toml", OS_CONFIG_TOML),
            ("rhel-9/OperatingSystem.toml", rhel_config),
        ]);

        let parsed = read_archive(archive.as_slice()).unwrap();
        assert_eq!(parsed.os_configs.len(), 2);
        assert!(parsed.os_configs.contains_key("ubuntu-2204"));
        assert!(parsed.os_configs.contains_key("rhel-9"));
        assert_eq!(parsed.os_configs["rhel-9"].name, "RHEL");
    }

    #[test]
    fn test_nested_os_config_not_parsed() {
        // An OperatingSystem.toml more than one level deep must be ignored.
        let archive = build_test_archive(&[
            ("manifest.toml", MANIFEST_TOML),
            ("ubuntu-2204/subdir/OperatingSystem.toml", OS_CONFIG_TOML),
        ]);

        let parsed = read_archive(archive.as_slice()).unwrap();
        assert!(
            parsed.os_configs.is_empty(),
            "nested OperatingSystem.toml should not be parsed as an OS config"
        );
    }
}
