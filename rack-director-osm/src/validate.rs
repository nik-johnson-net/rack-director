use std::fs;
use std::path::Path;

use anyhow::{Context, bail};

/// Run the validate subcommand: read an OSM archive and report validation errors.
pub fn run(file: &Path) -> anyhow::Result<()> {
    let data = fs::read(file).with_context(|| format!("failed to read '{}'", file.display()))?;

    let parsed = osm::read_archive(data.as_slice())
        .with_context(|| format!("failed to parse '{}'", file.display()))?;

    let errors = osm::validate_osm(&parsed);
    if errors.is_empty() {
        println!(
            "OK: '{}' is valid ({} OS configs)",
            file.display(),
            parsed.os_configs.len()
        );
        Ok(())
    } else {
        for err in &errors {
            eprintln!("ERROR: {err}");
        }
        bail!("{} validation error(s) found", errors.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osm::archive::tests_helper::build_test_archive;

    fn write_archive_to_tmp(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.osm");
        let data = build_test_archive(files);
        std::fs::write(&path, data).unwrap();
        dir
    }

    #[test]
    fn test_validate_valid_archive_succeeds() {
        let dir = write_archive_to_tmp(&[
            (
                "manifest.toml",
                br#"
name = "Test"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu"]
"#,
            ),
            (
                "ubuntu/OperatingSystem.toml",
                br#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#,
            ),
            ("ubuntu/vmlinuz", b"kernel"),
            ("ubuntu/initrd.img", b"initramfs"),
            ("ubuntu/install.sh", b"#!/bin/bash"),
        ]);
        let result = run(&dir.path().join("test.osm"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_archive_returns_error() {
        let dir = write_archive_to_tmp(&[
            (
                "manifest.toml",
                br#"
name = "Test"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu"]
"#,
            ),
            // Missing OperatingSystem.toml and all files for ubuntu
        ]);
        let result = run(&dir.path().join("test.osm"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_nonexistent_file_returns_error() {
        let result = run(Path::new("/nonexistent/file.osm"));
        assert!(result.is_err());
    }
}
