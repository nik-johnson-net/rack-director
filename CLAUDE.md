# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Overview

Rack Director provides low level server inventory and control using PXEBoot. The system is made up of two components: The server and the agent. The server (rack-director) contains the configuration and provides PXEBoot services via DHCP, TFTP, and HTTP. The HTTP component also serves a UI (rack-director-ui) and an API.

The Agent (rack-agent) is run on the server via PXEBoot to execute most detection and configuration functions. A full PXE-bootable image is created called agent-image that starts the rack-agent and reboots on its exit. It is included with docker images of rack-director (docker/).

A development tool named rack-simulator exists to simulate machine interactions when running rack-director locally, and also contains functions to run end-to-end tests using qemu.

Rust code that is shared between multiple projects is kept in common.

The `osm` library crate contains shared OSM types (Manifest, OperatingSystemConfig, archive parsing, validation) extracted from rack-director so they can be used by other tools without depending on rack-director internals.

The `rack-director-osm` binary crate is a CLI tool for building and validating OSM archives.

## Boot Process

A server that boots will first request a DHCP lease in both BIOS and UEFI modes, requesting boot options. Rack-director will respond with a DHCP lease containing boot options pointing to an iPXE image, which has more features and provides a common configuration across any device type. BIOS servers will load iPXE over TFTP, while UEFI may load iPXE over TFTP or HTTP. Once the server boots into iPXE, it will again request a DHCP Lease. This time the rack-director DHCP server will recognize the request as coming from iPXE firmware, and will instruct it to load a config from the rack-director HTTP server.

The iPXE config is dynamic and will be constructed based on what the machine should do next, either instructing it to boot into the rack-image, an os installer, local disk, or something else.

# Key Concepts

## Device
Devices are any server under the control of rack-director. Devices belong to a Platform, and may belong to a Role if provisioned. Devices have a lifecycle state: New, Unprovisioned, Provisioned, Broken, and Decommissioned. Devices contain attributes collected via the device-scan action in addition to user-defined configuration like kernel cmdline overrides.

## Platforms
Platforms group similar physical devices together, representing common hardware configurations (disks, NICs, CPUs, memory). They provide labels (ROOT, DATA1, NIC1) that Roles reference in disk layouts and templates. Devices are auto-assigned a Platform after hardware discovery based on matching hardware attributes. Platforms may optionally declare a `firmware_mode` (bios/uefi) that constrains which devices match them.

**Key Features:**
- Auto-detection on hardware discovery (matches by disk count/types/sizes ±5%, NIC count/speeds, CPU config, memory ±1 GiB)
- Automatic label assignment (ROOT = smallest+fastest disk, DATA1/DATA2 by bus order, NIC1/NIC2 by bus order)
- Manual platform assignment/override via UI/API
- Label resolution in disk layouts and templates using a two-level override system:
  1. Device-level override (`disk_label_overrides` on DeviceAttributes) — highest priority
  2. Canonical position matching (platform disk index → device disk index, sorted by type+size)
- Platform disk labels editable via `PUT /api/platforms/{id}/disks/{index}/label`
- `PlatformDisk` does **not** store a `path` field — paths always come from the device's own disk scan

See @.claude/docs/platforms.md for detailed platform documentation.

## Roles
Roles define how a Device should be configured. They define how the disks should be provisioned, and what operating system should be installed. Roles may optionally declare a `firmware_mode` (bios/uefi) that is validated against the device's detected `boot_mode` at role assignment time.

## Provisioning Workflow
Devices move through lifecycle states via multi-stage provisioning. The modern provisioning workflow includes:

1. **partition_disks** - Configure disk layout based on Role configuration (uses Platform labels if available)
2. **install_os** - Install operating system via PXE boot

See @.claude/docs/actions-reference.md for complete action documentation and workflow details.

## Firmware Mode
Devices are classified as `bios` or `uefi` based on whether `/sys/firmware/efi` exists at boot time (x86/x86_64 only). This is stored as `device.boot_mode` and flows through:
- **Device**: `boot_mode` attribute detected during `device-scan` (x86 only; None for other architectures)
- **Platform**: optional `firmware_mode` field — if set, only devices with a matching `boot_mode` will auto-match
- **Role**: optional `firmware_mode` field — if set, validated against device `boot_mode` at role assignment time

Install script templates can use `{{ device.boot_mode }}`, `{{ device.is_uefi }}`, `{{ device.is_bios }}`.

## Disk Layouts
Disk partition layouts are defined at the Role level and applied during the `partition_disks` action. Supports:
- Multiple devices with flexible sizing (fixed size, percentage, or "rest")
- Multiple filesystems (ext4, xfs, btrfs, vfat, swap)
- Partition flags (boot, esp, lvm)
- Device path resolution and validation
- **Platform labels** (ROOT, DATA1, etc.) that resolve to actual device paths

See @.claude/docs/actions-reference.md for disk layout configuration examples.

# Additional Sources

Note: You should prefer information from docs and sources.

- `iPXE`: https://ipxe.org/docs
- `DHCP`: https://www.rfc-editor.org/rfc/rfc2131
- `TFTP`: https://www.rfc-editor.org/rfc/rfc1350

# Code Style

- Public functions should have exhaustive tests - both success and failure modes.
- Code should be modular with minimal public functions and classes.
- Add documentation for public functions.
- Follow Single Responsibility Principle. Modules should provide one API, and implementation of that API should be broken out into submodules.
- Functions more than 50 lines of code should be broken up into multiple functions.

# Commands

```bash
# Build all packages
cargo build

# Run all tests
cargo test

# Start development server (rack-director on 127.0.0.1:3000)
make devserver

# Build UI (from rack-director-ui/)
npm run build

# Build an OSM from the current directory
cargo run -p rack-director-osm -- build

# Validate an OSM archive
cargo run -p rack-director-osm -- validate <file.osm>
```

# Gotchas

- **Rust edition 2024**: `gen` is a reserved keyword. Use `rng.gen_range(...)` not `rng.gen::<T>()`.

# Coding Process

- ALWAYS ask clarifying questions if the prompt is ambiguous or contains an incorrect assumption.
- ALWAYS use the rust-implementer subagent for writing rust code.
- For ambiguous tasks, use Plan mode to explore codebase and understand context first.
- Brainstorm 2-3 solutions and present options via AskUserQuestion. Pick best solution only if clearly superior, otherwise let user choose.
- Code Review yourself
- No task is complete until it has been verified.
- ALWAYS update CLAUDE.md and other docs in .claude/docs
