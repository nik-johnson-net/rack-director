use std::collections::HashSet;
use std::fmt;

use crate::archive::ParsedArchive;

/// Describes a single validation problem found in a `ParsedArchive`.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    /// Human-readable source location (e.g., `"ubuntu/OperatingSystem.toml"`).
    pub location: String,
    /// A short description of what is wrong.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Validates a `ParsedArchive` and returns all detected problems.
///
/// Checks performed:
/// 1. Every OS directory listed in `manifest.operating_systems` has a corresponding
///    `OperatingSystem.toml` that was successfully parsed (`os_configs`).
/// 2. Every entry in `os_configs` is listed in `manifest.operating_systems` (no orphans).
/// 3. For each architecture in each OS config, the `kernel`, `initramfs`,
///    `install_template`, and every `module` path resolves to an entry in `file_inventory`.
/// 4. No two OS configs share the same `(name, release, arch)` tuple (case-insensitive).
///
/// Template content is intentionally NOT validated here — that requires re-reading the
/// archive (which may be 10 GiB) and is deferred to the extraction phase (Milestone 3).
pub fn validate_osm(parsed: &ParsedArchive) -> Vec<ValidationError> {
    let inventory: HashSet<&str> = parsed.file_inventory.iter().map(String::as_str).collect();
    let mut errors: Vec<ValidationError> = Vec::new();

    validate_manifest_os_consistency(parsed, &mut errors);
    validate_file_references(parsed, &inventory, &mut errors);
    validate_no_duplicate_os_arch(parsed, &mut errors);

    errors
}

// ---------------------------------------------------------------------------
// Internal validation steps
// ---------------------------------------------------------------------------

/// Checks that every OS dir in the manifest has a parsed config and vice-versa.
fn validate_manifest_os_consistency(parsed: &ParsedArchive, errors: &mut Vec<ValidationError>) {
    for os_dir in &parsed.manifest.operating_systems {
        if !parsed.os_configs.contains_key(os_dir.as_str()) {
            errors.push(ValidationError {
                location: format!("{os_dir}/OperatingSystem.toml"),
                message: format!(
                    "OS directory '{os_dir}' is listed in manifest but has no OperatingSystem.toml"
                ),
            });
        }
    }

    let manifest_set: HashSet<&str> = parsed
        .manifest
        .operating_systems
        .iter()
        .map(String::as_str)
        .collect();

    for os_dir in parsed.os_configs.keys() {
        if !manifest_set.contains(os_dir.as_str()) {
            errors.push(ValidationError {
                location: format!("{os_dir}/OperatingSystem.toml"),
                message: format!(
                    "OS directory '{os_dir}' has an OperatingSystem.toml but is not listed in manifest"
                ),
            });
        }
    }
}

/// Checks that every file referenced by each OS config exists in the inventory.
fn validate_file_references(
    parsed: &ParsedArchive,
    inventory: &HashSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for (os_dir, os_config) in &parsed.os_configs {
        let location = format!("{os_dir}/OperatingSystem.toml");
        for arch in &os_config.architectures {
            check_file_reference(&location, os_dir, &arch.kernel, "kernel", inventory, errors);
            check_file_reference(
                &location,
                os_dir,
                &arch.initramfs,
                "initramfs",
                inventory,
                errors,
            );
            check_file_reference(
                &location,
                os_dir,
                &arch.install_template,
                "install_template",
                inventory,
                errors,
            );
            for module in &arch.modules {
                check_file_reference(&location, os_dir, module, "module", inventory, errors);
            }
        }
    }
}

/// Emits a `ValidationError` if `{os_dir}/{filename}` is absent from the inventory.
fn check_file_reference(
    location: &str,
    os_dir: &str,
    filename: &str,
    field: &str,
    inventory: &HashSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let path = format!("{os_dir}/{filename}");
    if !inventory.contains(path.as_str()) {
        errors.push(ValidationError {
            location: location.to_owned(),
            message: format!("{field} file not found in archive: '{path}'"),
        });
    }
}

/// Checks that no two architecture configs share the same `(name, release, arch)` tuple
/// (comparison is case-insensitive).
fn validate_no_duplicate_os_arch(parsed: &ParsedArchive, errors: &mut Vec<ValidationError>) {
    // (lowercase name, lowercase release, lowercase arch) → first seen location
    let mut seen: std::collections::HashMap<(String, String, String), String> =
        std::collections::HashMap::new();

    for (os_dir, os_config) in &parsed.os_configs {
        let location = format!("{os_dir}/OperatingSystem.toml");
        for arch in &os_config.architectures {
            let key = (
                os_config.name.to_lowercase(),
                os_config.release.to_lowercase(),
                arch.arch.to_lowercase(),
            );
            if let Some(first_location) = seen.get(&key) {
                errors.push(ValidationError {
                    location: location.clone(),
                    message: format!(
                        "duplicate OS/arch tuple ({name}, {release}, {arch}) already defined in '{first}'",
                        name = os_config.name,
                        release = os_config.release,
                        arch = arch.arch,
                        first = first_location
                    ),
                });
            } else {
                seen.insert(key, location.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::read_archive;
    use crate::archive::tests_helper::build_test_archive;

    // ---------------------------------------------------------------------------
    // Shared fixtures
    // ---------------------------------------------------------------------------

    const MANIFEST_SINGLE: &[u8] = br#"
name = "Test Module"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu-2204"]
"#;

    const OS_UBUNTU: &[u8] = br#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;

    fn parse(files: &[(&str, &[u8])]) -> ParsedArchive {
        let archive = build_test_archive(files);
        read_archive(archive.as_slice()).expect("test archive should parse cleanly")
    }

    // ---------------------------------------------------------------------------
    // Happy path
    // ---------------------------------------------------------------------------

    #[test]
    fn test_valid_archive_no_errors() {
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            ("ubuntu-2204/vmlinuz", b"kernel"),
            ("ubuntu-2204/initrd.img", b"initramfs"),
            ("ubuntu-2204/install.sh", b"#!/bin/bash"),
        ]);

        let errors = validate_osm(&parsed);
        assert!(
            errors.is_empty(),
            "expected no errors, got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    // ---------------------------------------------------------------------------
    // Manifest consistency
    // ---------------------------------------------------------------------------

    #[test]
    fn test_missing_os_directory_listed_in_manifest() {
        // manifest says ubuntu-2204 exists but there is no OperatingSystem.toml for it.
        let manifest: &[u8] = br#"
name = "Test"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu-2204"]
"#;
        let parsed = parse(&[("manifest.toml", manifest)]);
        let errors = validate_osm(&parsed);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("listed in manifest"));
    }

    #[test]
    fn test_extra_os_directory_not_in_manifest() {
        // manifest lists ubuntu-2204; rhel-9 has an OperatingSystem.toml but is not in manifest.
        let manifest: &[u8] = br#"
name = "Test"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu-2204"]
"#;
        let rhel: &[u8] = br#"
name = "RHEL"
release = "9"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let parsed = parse(&[
            ("manifest.toml", manifest),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            ("ubuntu-2204/vmlinuz", b"k"),
            ("ubuntu-2204/initrd.img", b"i"),
            ("ubuntu-2204/install.sh", b"s"),
            ("rhel-9/OperatingSystem.toml", rhel),
            ("rhel-9/vmlinuz", b"k"),
            ("rhel-9/initrd.img", b"i"),
            ("rhel-9/install.sh", b"s"),
        ]);
        let errors = validate_osm(&parsed);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not listed in manifest"));
    }

    // ---------------------------------------------------------------------------
    // File reference checks
    // ---------------------------------------------------------------------------

    #[test]
    fn test_missing_kernel_file() {
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            // vmlinuz is absent
            ("ubuntu-2204/initrd.img", b"i"),
            ("ubuntu-2204/install.sh", b"s"),
        ]);
        let errors = validate_osm(&parsed);
        assert!(errors.iter().any(|e| e.message.contains("vmlinuz")));
    }

    #[test]
    fn test_missing_initramfs_file() {
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            ("ubuntu-2204/vmlinuz", b"k"),
            // initrd.img is absent
            ("ubuntu-2204/install.sh", b"s"),
        ]);
        let errors = validate_osm(&parsed);
        assert!(errors.iter().any(|e| e.message.contains("initrd.img")));
    }

    #[test]
    fn test_missing_install_template_file() {
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            ("ubuntu-2204/vmlinuz", b"k"),
            ("ubuntu-2204/initrd.img", b"i"),
            // install.sh is absent
        ]);
        let errors = validate_osm(&parsed);
        assert!(errors.iter().any(|e| e.message.contains("install.sh")));
    }

    #[test]
    fn test_missing_module_file() {
        let os_config_with_module: &[u8] = br#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
modules = ["squashfs.ko"]
install_template = "install.sh"
"#;
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", os_config_with_module),
            ("ubuntu-2204/vmlinuz", b"k"),
            ("ubuntu-2204/initrd.img", b"i"),
            ("ubuntu-2204/install.sh", b"s"),
            // squashfs.ko is absent
        ]);
        let errors = validate_osm(&parsed);
        assert!(errors.iter().any(|e| e.message.contains("squashfs.ko")));
    }

    // ---------------------------------------------------------------------------
    // Duplicate OS/arch detection
    // ---------------------------------------------------------------------------

    #[test]
    fn test_duplicate_os_name_release_arch() {
        // Two OS dirs that define the same (name, release, arch) tuple.
        let manifest: &[u8] = br#"
name = "Test"
version = "1.0.0"
author = "Test"
description = "Test"
operating_systems = ["ubuntu-a", "ubuntu-b"]
"#;
        // Both declare Ubuntu 22.04 x86-64.
        let ubuntu_a: &[u8] = br#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let ubuntu_b: &[u8] = br#"
name = "ubuntu"
release = "22.04"

[[architectures]]
arch = "X86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let parsed = parse(&[
            ("manifest.toml", manifest),
            ("ubuntu-a/OperatingSystem.toml", ubuntu_a),
            ("ubuntu-a/vmlinuz", b"k"),
            ("ubuntu-a/initrd.img", b"i"),
            ("ubuntu-a/install.sh", b"s"),
            ("ubuntu-b/OperatingSystem.toml", ubuntu_b),
            ("ubuntu-b/vmlinuz", b"k"),
            ("ubuntu-b/initrd.img", b"i"),
            ("ubuntu-b/install.sh", b"s"),
        ]);
        let errors = validate_osm(&parsed);
        assert!(
            errors.iter().any(|e| e.message.contains("duplicate")),
            "expected a duplicate error, got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    // ---------------------------------------------------------------------------
    // Multiple errors accumulated
    // ---------------------------------------------------------------------------

    #[test]
    fn test_multiple_errors_accumulated() {
        // Missing kernel AND missing initramfs — both should be reported.
        let parsed = parse(&[
            ("manifest.toml", MANIFEST_SINGLE),
            ("ubuntu-2204/OperatingSystem.toml", OS_UBUNTU),
            // Both vmlinuz and initrd.img are absent; only install.sh present.
            ("ubuntu-2204/install.sh", b"s"),
        ]);
        let errors = validate_osm(&parsed);
        assert!(
            errors.len() >= 2,
            "expected at least 2 errors, got {}: {:?}",
            errors.len(),
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }
}
