use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Create a zstd-compressed tar archive from `build_dir` and write it to `output_path`.
///
/// All files in `build_dir` are added with paths relative to `build_dir`.
pub fn create_archive(build_dir: &Path, output_path: &Path) -> Result<()> {
    let output_file = fs::File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;
    let encoder = zstd::Encoder::new(output_file, 3).context("failed to create zstd encoder")?;
    let mut tar_builder = tar::Builder::new(encoder);

    add_dir_recursive(&mut tar_builder, build_dir, build_dir)?;

    let encoder = tar_builder
        .into_inner()
        .context("failed to finalize tar archive")?;
    encoder
        .finish()
        .context("failed to finalize zstd compression")?;

    Ok(())
}

/// Recursively add all files under `current` to the tar archive,
/// with paths relative to `base`.
fn add_dir_recursive<W: Write>(
    builder: &mut tar::Builder<W>,
    base: &Path,
    current: &Path,
) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read directory {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let rel_path = path
            .strip_prefix(base)
            .context("path is not under base directory")?;

        if path.is_dir() {
            add_dir_recursive(builder, base, &path)?;
        } else {
            builder
                .append_path_with_name(&path, rel_path)
                .with_context(|| format!("failed to add {} to archive", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_build_dir(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }
        dir
    }

    #[test]
    fn test_create_archive_roundtrip() {
        let dir = setup_build_dir(&[
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

        let output = dir.path().join("output.osm");
        create_archive(dir.path(), &output).unwrap();
        assert!(output.exists());

        // Verify the archive can be parsed and validated
        let data = fs::read(&output).unwrap();
        let parsed = osm::read_archive(data.as_slice()).unwrap();
        assert_eq!(parsed.manifest.name, "Test");
        assert!(parsed.os_configs.contains_key("ubuntu"));

        let errors = osm::validate_osm(&parsed);
        assert!(errors.is_empty(), "errors: {:?}", errors);
    }

    #[test]
    fn test_create_archive_contains_all_files() {
        let dir = setup_build_dir(&[
            (
                "manifest.toml",
                br#"
name = "M"
version = "0.1.0"
author = "T"
description = "T"
operating_systems = []
"#,
            ),
            ("some-dir/file-a.bin", b"aaa"),
            ("some-dir/file-b.bin", b"bbb"),
        ]);

        let output = dir.path().join("output.osm");
        create_archive(dir.path(), &output).unwrap();

        let data = fs::read(&output).unwrap();
        let parsed = osm::read_archive(data.as_slice()).unwrap();
        assert!(parsed.file_inventory.contains(&"manifest.toml".to_string()));
        assert!(
            parsed
                .file_inventory
                .contains(&"some-dir/file-a.bin".to_string())
        );
        assert!(
            parsed
                .file_inventory
                .contains(&"some-dir/file-b.bin".to_string())
        );
    }
}
