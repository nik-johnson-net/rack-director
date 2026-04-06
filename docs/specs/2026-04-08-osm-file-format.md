# OSM File Format Specification

## Overview

An Operating System Module (OSM) file is a zstd-compressed tar archive (`.tar.zst`) that packages one or more operating system configurations for use with Rack Director. Each OSM file contains a manifest describing the module, plus per-OS subdirectories with boot files and install templates.

## Archive Format

- **Compression:** zstd (any compression level)
- **Container:** tar
- **File extension:** `.tar.zst` (conventional, not enforced)

### Structure

```
module.tar.zst
├── manifest.toml
├── <os-dir>/
│   ├── OperatingSystem.toml
│   ├── <kernel>
│   ├── <initramfs>
│   ├── <install-template>
│   └── <modules...>
└── <os-dir>/
    ├── OperatingSystem.toml
    └── ...
```

### Path Rules

- `manifest.toml` must be at the archive root (not nested in a subdirectory).
- `OperatingSystem.toml` must be exactly one level deep: `{os-dir}/OperatingSystem.toml`.
- Leading `./` on paths is normalized automatically and has no effect.
- Directory entries in the tar archive are ignored; only file entries are considered.
- All TOML files must be valid UTF-8.

## manifest.toml

The root manifest declares module metadata and lists the OS directories it contains.

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Module name. Must be unique across modules (compared case-insensitively). |
| `version` | String | Yes | Semantic version (e.g., `"1.0.0"`). Must be valid [SemVer 2.0](https://semver.org/). |
| `author` | String | Yes | Module author or organization. |
| `description` | String | Yes | Human-readable description of the module. |
| `operating_systems` | Array of String | Yes | List of OS subdirectory names present in the archive. May be empty. |

### Example

```toml
name = "Default"
version = "1.0.0"
author = "Rack Director Project"
description = "Default operating system module"
operating_systems = ["ubuntu-2204", "rhel-9"]
```

## OperatingSystem.toml

Each OS subdirectory listed in the manifest must contain an `OperatingSystem.toml` file defining the OS metadata, architecture-specific boot configuration, and optional template variables.

### Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Human-readable OS name (e.g., `"Ubuntu"`). |
| `release` | String | Yes | OS version or release identifier (e.g., `"22.04"`). |
| `architectures` | Array of ArchitectureConfig | Yes | One or more architecture-specific configurations. |
| `template_variables` | Array of TemplateVariable | No | User-configurable variables for install templates. Defaults to empty. |

### ArchitectureConfig

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `arch` | String | Yes | Architecture identifier (e.g., `"x86-64"`, `"aarch64"`). |
| `kernel` | String | Yes | Kernel image filename, relative to this OS directory. |
| `initramfs` | String | Yes | Initramfs image filename, relative to this OS directory. |
| `install_template` | String | Yes | Handlebars install template filename, relative to this OS directory. |
| `modules` | Array of String | No | Kernel module filenames, relative to this OS directory. Defaults to empty. |
| `cmdline` | String | No | OS-level kernel command line arguments. Defaults to empty string. |

### TemplateVariable

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Variable name, referenced in install templates. |
| `type` | String | Yes | Data type. One of: `"string"`, `"list"`, `"boolean"`, `"integer"`. |
| `description` | String | Yes | Human-readable description of the variable. |
| `required` | Boolean | No | Whether the variable must be provided at role assignment. Defaults to `false`. |
| `default` | Any | No | Default value when not provided. Type should match the declared `type`. |

### Full Example

```toml
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
modules = ["squashfs", "overlay"]
cmdline = "quiet splash"
install_template = "ubuntu-2204.sh.hbs"

[[architectures]]
arch = "aarch64"
kernel = "vmlinuz-arm64"
initramfs = "initrd-arm64.img"
install_template = "ubuntu-2204-arm64.sh.hbs"

[[template_variables]]
name = "root_password"
type = "string"
description = "Root user password"
required = true

[[template_variables]]
name = "dns_servers"
type = "list"
description = "DNS server addresses"
default = ["8.8.8.8", "8.8.4.4"]

[[template_variables]]
name = "enable_ssh"
type = "boolean"
description = "Enable SSH daemon on first boot"
default = true
```

### Minimal Example

```toml
name = "Minimal OS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
```

## File References

All file references in `OperatingSystem.toml` (`kernel`, `initramfs`, `install_template`, and entries in `modules`) are relative to the OS subdirectory. The resolved path in the archive is `{os-dir}/{filename}`.

For example, if `ubuntu-2204/OperatingSystem.toml` declares `kernel = "vmlinuz"`, the archive must contain the file `ubuntu-2204/vmlinuz`.

## Validation Rules

All of the following rules must pass for an OSM file to be accepted. Validation is exhaustive: all errors are collected and reported together rather than failing on the first error.

1. **manifest.toml exists** — The archive must contain a `manifest.toml` at the root level.
2. **Valid TOML** — Both `manifest.toml` and all `OperatingSystem.toml` files must be valid UTF-8 and parseable as TOML conforming to the schemas above.
3. **Valid SemVer** — The `version` field in `manifest.toml` must be a valid [Semantic Version 2.0](https://semver.org/).
4. **Manifest-directory consistency** — Every entry in `operating_systems` must have a corresponding `{dir}/OperatingSystem.toml` in the archive, and every `OperatingSystem.toml` found one level deep must be listed in `operating_systems`.
5. **File references exist** — For each architecture in each OS config, the `kernel`, `initramfs`, `install_template`, and every entry in `modules` must resolve to a file present in the archive at `{os-dir}/{filename}`.
6. **No duplicate OS/arch tuples** — No two OS configurations across the entire archive may share the same `(name, release, arch)` tuple. Comparison is case-insensitive.
