# rack-agent Documentation

## Overview

The rack-agent is a lightweight Rust agent that runs on PXE-booted devices to perform hardware discovery, configuration, and provisioning tasks. It communicates with rack-director via HTTP to fetch configuration and report status.

The agent is designed to run in a minimal Linux environment (initramfs) and performs atomic operations that move devices through their lifecycle.

## Architecture

### Source Structure

```
rack-agent/src/
├── main.rs         # Entry point, command routing, cmdline parsing
├── client.rs       # HTTP client for rack-director communication
├── bmc.rs          # BMC detection, configuration, and management
├── scan.rs         # Hardware scanning (CPU, memory, disks, NICs)
├── lvm.rs          # Shared LVM/dm cleanup (remove_lvm_on_disks), used by partition.rs and wipe.rs
├── partition.rs    # Disk partitioning logic
├── wipe.rs         # Secure disk erasure (shred, ATA secure erase, nvme format)
└── daemon.rs       # Daemon mode: poll loop and action dispatch
```

### Key Components

#### `main.rs` - Entry Point
- Parses command-line arguments and kernel cmdline
- Initializes logging (logfmt format)
- Resolves rack-director URL from `--director-url` flag or `/proc/cmdline` (`rackdirector.url=`)
- Resolves action from `--action` flag or `/proc/cmdline` (`rackdirector.action=`)
- Routes to appropriate command handler
- Exits with code 10 on error (for debugging)

#### `client.rs` - Rack Director Client
- HTTP client for communicating with rack-director's `/cnc/` endpoints
- Handles:
  - Updating device attributes
  - Reporting action success/failure
  - Fetching BMC configuration
  - Fetching disk layout configuration
  - Polling for pending actions (`poll()` → `GET /cnc/poll`)
- `PollAction` and `ServerMessage` types define the daemon wire protocol

#### `bmc.rs` - BMC Management
- BMC detection using dynamic LAN channel discovery
- Channel detection via `ipmitool channel info` (queries channels 1-16)
- BMC configuration (static IP or DHCP)
- IPMI user management (create/update users with admin privileges)
- Automatic fallback to default channels [1, 2, 8] if detection fails

#### `scan.rs` - Hardware Scanning
- Device hardware discovery using SMBIOS/DMI tables
- Network interface detection
- Disk enumeration from `/dev/disk/by-path/`
- CPU and memory information gathering
- Delegates BMC scanning to `bmc.rs` module
- `list_disk_paths()` — public helper that returns the by-path of every whole disk (used by `partition.rs` for whole-machine pre-wipe)
- `list_wipe_targets()` — public helper that returns every whole disk as a `WipeTarget` (device node, name, detected `DiskType`), used by `wipe.rs` for the `WipeDisks` action
- `list_real_block_device_names()` — public helper that lists every non-virtual device name under `/sys/block` (excludes `loop*`/`ram*`/`zram*`/`dm-*`/`sr*`/`md*`/`nbd*`); used by `wipe.rs` as a cross-check against `list_wipe_targets()`'s by-path enumeration, which can silently miss real disks that lack a by-path symlink (e.g. `mmcblk*` on systemd >= 254, systemd issue #28671). `md*`/`dm-*` are excluded because they are virtual aggregates whose member disks are enumerated separately (relevant when a previous OS's mdraid array gets auto-assembled by the agent's own initramfs)

#### `lvm.rs` - LVM/dm Cleanup
- `remove_lvm_on_disks(disk_paths: &[String])` — best-effort: deactivates and removes LVM volume groups whose PVs reside on the given disks (`vgchange -an` + `vgremove --force`), so residual LVM/dm state doesn't hold a disk busy for a subsequent erase or partition operation. An empty slice means "every VG on the system" (used for whole-machine operations). Every failure (the `pvs` query itself, or an individual `vgchange`/`vgremove`) is logged as a warning and does not stop the cleanup.
- Shared by `partition.rs` (step 0 of `apply_disk_layout`, scoped to the layout's disks, or system-wide when `wipe_all_disks` is set) and `wipe.rs` (step 0 of `wipe_disks`, always system-wide, since `wipe_disks` always targets every whole disk on the machine)

#### `partition.rs` - Disk Partitioning
- Multi-stage disk provisioning (optional pre-wipe all, wipe, partition, LVM, ZFS, format, **verify**)
- Partition table creation (GPT or MBR)
- Disk partitioning with flexible sizing (fixed, percentage, rest)
- 1MiB-aligned partition offsets
- LVM volume group and logical volume setup
- ZFS pool and dataset creation
- Filesystem formatting (ext4, xfs, btrfs, vfat, swap)
- Partition flag management (boot, esp, lvm)
- SATA and NVMe partition path generation
- Post-apply verification via `sfdisk --json` and `vgs --noheadings`
- Optional whole-machine pre-wipe via `DiskLayout.wipe_all_disks`
- Step 0 of `apply_disk_layout` calls `lvm::remove_lvm_on_disks()` before any wiping happens

## Supported Commands

### 1. DeviceScan

**Purpose:** Scans device hardware and uploads metadata to rack-director

**CLI Usage:**
```bash
rack-agent --director-url http://rack-director:3000/cnc device-scan
```

**Kernel Cmdline Usage:**
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=device-scan
```

**What it scans:**
- Device UUID (from SMBIOS)
- Manufacturer, product name, serial number
- BIOS version and vendor
- CPU information (designation, manufacturer, cores, threads, speed)
- Memory information (size, speed, manufacturer, part number)
- Network interfaces (MAC addresses, IP addresses, link speed)
- BMC information (MAC address, IP, IP source)
- Disk information (name, size, type, model, path, serial, vendor, uuid/WWN)

**Output:** Uploads device attributes to `/cnc/update_attributes`

**Example Attributes:**
```json
{
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "manufacturer": "Dell Inc.",
  "product_name": "PowerEdge R740",
  "serial_number": "ABC123",
  "bios_version": "2.10.0",
  "network_interfaces": [
    {
      "interface_name": "eth0",
      "mac_address": "00:11:22:33:44:55",
      "ip_address": "10.0.0.100",
      "speed_mbps": 10000
    }
  ],
  "disks": [
    {
      "name": "sda",
      "size": "1TB",
      "disk_type": "ssd",
      "model": "Samsung 970 EVO",
      "path": "/dev/disk/by-path/pci-0000:00:1f.2-nvme-1",
      "serial": "S1234ABCD",
      "vendor": "Samsung",
      "uuid": "eui.002538b311b2399a"
    }
  ]
}
```

**Disk scan fields:**

| Field | Source |
|-------|--------|
| `name` | `/sys/block/` enumeration |
| `size` | `lsblk` |
| `disk_type` | `lsblk` rotational flag |
| `model` | `lsblk` |
| `path` | `/dev/disk/by-path/` symlink |
| `serial` | `/sys/block/{name}/device/serial` (trimmed) |
| `vendor` | `/sys/block/{name}/device/vendor` (trimmed) |
| `uuid` | `/dev/disk/by-id/wwn-*` symlink or udevadm `ID_WWN` |

All disk fields except `name` are `Option` — absent for virtual or unusual devices.

**Implementation:** `rack-agent/src/scan.rs::device_scan()`

**Testing:**
```bash
# Dry-run mode (no upload)
rack-agent --director-url http://localhost:3000/cnc device-scan --no-upload
```

---

### 2. ConfigureBmc

**Purpose:** Configures BMC (Baseboard Management Controller) with IP and credentials

**CLI Usage:**
```bash
rack-agent --director-url http://rack-director:3000/cnc configure-bmc
```

**Kernel Cmdline Usage:**
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=configure-bmc
```

**What it does:**
1. Gets device UUID from SMBIOS
2. Fetches BMC configuration from rack-director (`GET /cnc/devices/{uuid}/bmc_config`)
3. Detects available BMC LAN channels dynamically using `ipmitool channel info`
4. Configures BMC IP address (static or DHCP) on detected LAN channels
5. Sets BMC credentials (username/password)
6. Reports success or failure to rack-director

**Dynamic Channel Detection:**
- Queries channels 1-16 using `ipmitool channel info`
- Identifies LAN channels by looking for "802.3 LAN" medium type
- Tries configuration on each detected LAN channel in order
- Falls back to [1, 2, 8] if detection fails or ipmitool unavailable
- Fixes "Channel X is not a LAN channel!" errors on systems with non-standard channel layouts

**BMC Configuration Format:**
```json
{
  "ip_address_source": "static",
  "ip_address": "10.0.1.100",
  "netmask": "255.255.255.0",
  "gateway": "10.0.1.1",
  "username": "admin",
  "password": "secret123"
}
```

**Implementation:** `rack-agent/src/bmc.rs::bmc_configure()`

**Key Features:**
- Dynamic channel detection prevents hardcoded channel errors
- Automatic fallback ensures compatibility with all systems
- Tries all detected channels until one succeeds
- Comprehensive error reporting for troubleshooting

---

### 3. PartitionDisks

**Purpose:** Partitions disks, creates LVM volume groups, and ZFS pools based on role configuration

**CLI Usage:**
```bash
rack-agent --director-url http://rack-director:3000/cnc partition-disks
```

**Kernel Cmdline Usage:**
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=partition-disks
```

**What it does:**
1. Gets device UUID from SMBIOS
2. Fetches resolved disk layout from rack-director (`GET /cnc/devices/{uuid}/disk_layout`)
   - Platform labels are already resolved to device paths by rack-director
3. Applies layout in up to 6 stages:
   - **Stage 0 (optional):** If `wipe_all_disks: true`, erase partition info from **every** whole disk on the machine via `wipefs --all --force` + `sgdisk --zap-all` (uses `scan::list_disk_paths()` to enumerate targets). Disks in the layout are re-wiped in Stage 1; this is intentional and idempotent.
   - **Stage 1:** Wipe and partition each disk in the layout (wipefs, sgdisk, parted mklabel/mkpart, partprobe)
   - **Stage 2:** Wait for udev to settle
   - **Stage 3:** Set up LVM volume groups (pvcreate, vgcreate, lvcreate, mkfs)
   - **Stage 4:** Set up ZFS pools and datasets (zpool create, zfs create)
   - **Stage 5:** Format simple partitions (not LVM/ZFS) with mkfs
4. **Verifies** the layout was applied correctly:
   - Runs `sfdisk --json` on each disk to confirm partition tables are readable
   - Runs `vgs --noheadings` to confirm all configured LVM VGs exist
5. Reports success or failure to rack-director (verification failure also reports failure)

**Disk Layout Format:**
```json
{
  "disks": [
    {
      "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
      "partition_table": "gpt",
      "partitions": [
        {
          "label": "efi",
          "size": "512MiB",
          "filesystem": "vfat",
          "mount_point": "/boot/efi",
          "flags": ["esp", "boot"]
        },
        {
          "label": "root",
          "size": "rest",
          "filesystem": "ext4",
          "mount_point": "/"
        }
      ]
    }
  ],
  "wipe_all_disks": true
}
```

**`wipe_all_disks` field (optional, default `false`):**
When `true`, the agent erases partition info (`wipefs --all --force` + `sgdisk --zap-all`) from **every** whole disk on the machine **before** applying the layout. This ensures no stale partition metadata remains on disks not listed in the layout (e.g., extra data disks from a previous use). Disks listed in `disks` are wiped again during Stage 1; running the commands twice is idempotent and harmless. When `false` (default), only the disks listed in `disks` are wiped.

**Supported Features:**
- **Size formats:** Binary (`512MiB`, `100GiB`), decimal (`500GB`), shorthand (`50G`), percentage (`50%`), rest
- **Filesystems:** ext2, ext3, ext4, xfs, btrfs, vfat, swap
- **Raw partitions:** A partition may omit its filesystem (`filesystem: null`) to be defined but left unformatted. Stage 5 skips formatting it, just as a raw LVM logical volume (e.g. a Ceph OSD) is left unformatted. Such a partition must not declare a `mount_point`.
- **Partition flags:** boot, esp, lvm
- **Multiple devices:** Can partition multiple disks in one layout
- **LVM:** Volume groups with logical volumes, supports `100%FREE`/`rest` for last LV
- **ZFS:** Pools with configurable vdev types (single, mirror, raidz, raidz2), datasets, and zvols
- **Alignment:** All partitions are 1MiB-aligned for optimal performance

**Implementation:** `rack-agent/src/partition.rs::partition_disks()`

**System Dependencies:**
- `wipefs`, `sgdisk`, `parted`, `partprobe`, `udevadm` - Disk operations
- `mkfs.ext4`, `mkfs.xfs`, `mkfs.btrfs`, `mkfs.vfat`, `mkswap` - Filesystem formatting
- `pvcreate`, `vgcreate`, `lvcreate` - LVM operations (optional, for LVM layouts)
- `zpool`, `zfs` - ZFS operations (optional, for ZFS layouts)
- `lsblk` - Disk size detection

**See Also:** @.claude/docs/actions-reference.md for disk layout examples

---

### 4. WipeDisks

**Purpose:** Securely erases every whole disk on the machine, using the method appropriate to each disk's type

**CLI Usage:**
```bash
rack-agent --director-url http://rack-director:3000/cnc wipe-disks
```

**Kernel Cmdline Usage:**
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=wipe-disks
```

**What it does:**
1. Gets device UUID from SMBIOS
2. Enumerates every whole disk on the machine (`scan::list_wipe_targets()` — same by-path enumeration and type detection used by `device-scan`/`partition-disks`), then validates that enumeration against reality (see **Fail-on-incomplete-enumeration guard** below)
3. Deactivates any LVM volume groups on the machine (`lvm::remove_lvm_on_disks(&[])`, system-wide — see **LVM/dm cleanup** below)
4. Erases every disk **concurrently**, each with the method hardcoded to its detected type (not configurable):

| Disk Type | Method |
|-----------|--------|
| HDD | `shred -v -n1 <dev>` — single random-data pass |
| SATA SSD | ATA Secure Erase via `hdparm`. Reads `hdparm -I <dev>` to check security state, then sets a user password (`hdparm --user-master u --security-set-pass p <dev>`) and erases with `--security-erase-enhanced` if the drive reports enhanced erase support, else `--security-erase` |
| NVMe | `nvme format <dev> --ses=2 --force` (cryptographic erase) if `nvme id-ctrl <dev>`'s `fna` field has bit 2 (`0x4`) set, else `--ses=1 --force` (user data erase) |

5. Reports success or failure to rack-director

**LVM/dm cleanup:** Before erasing, the agent deactivates and removes every LVM volume group on the machine via the shared `lvm::remove_lvm_on_disks()` helper (`vgchange -an` + `vgremove --force`, best-effort). This exists because `nvme format` reliably refuses with "Device or resource busy" when a namespace has an open handle (mount, LVM, or swap), and `hdparm --security-erase` similarly misbehaves with active dm holders — and per `lvmautoactivation(7)`, transient `lvm2-pvscan@X:Y.service` units spawned by the `69-dm-lvm.rules` udev rule can re-activate a VG on disk rescan even with `lvm2-monitor`/`lvm-activate-boot` masked. Cleanup failures are logged as warnings and do not block the wipe from proceeding — the erase commands themselves will surface a clear busy/failure error if stale LVM state actually blocks them.

**Fail-on-incomplete-enumeration guard:** `wipe_disks` never reports success having wiped fewer disks than actually exist. It fails immediately, before touching any disk, if:
- `scan::list_wipe_targets()` (by-path enumeration) returns **zero** disks, or
- a cross-check against `scan::list_real_block_device_names()` (`/sys/block`, excluding `loop*`/`ram*`/`zram*`/`dm-*`/`sr*`/`md*`/`nbd*`) finds a real whole disk that by-path enumeration missed

The second case matters because `/dev/disk/by-path/` symlinks are not guaranteed for every device — e.g. systemd >= 254 does not create them for `mmcblk*` (onboard eMMC), so that disk would otherwise be silently skipped (systemd issue #28671). The error names every missing device. `md*` (software RAID) and `nbd*` (network block devices) are excluded because they are virtual aggregates, not physical disks: their member disks are enumerated separately under `/sys/block`. This matters on decommissioned machines whose previous OS used mdraid — the agent's own initramfs can auto-assemble the array (the same class of udev auto-activation that motivates the LVM cleanup above), which would otherwise make the guard mistake the resulting `md0`/`md127` for an unwiped real disk and abort a healthy wipe.

**Concurrent, all-attempted wiping:** All target disks are wiped concurrently (`tokio::task::JoinSet`) rather than sequentially — they are independent block devices with no shared kernel state between their `shred`/`hdparm`/`nvme` invocations, so this avoids an ~Nx slowdown on a chassis with many bays. A failure on one disk does not stop the others: every disk is attempted, and only after all have finished does the action fail, with an aggregate error naming every disk that failed and why. This matters because each erase pass can take hours — leaving unattempted disks after an early failure would mean a retry restarts work that didn't need to be redone. Log lines from the long-running erase commands are tagged with the device node so they stay attributable when multiple disks' output interleaves.

**SATA SSD fail-hard cases:** If `hdparm -I` reports the drive as **frozen**, or reports the ATA security feature set as **not supported**, the action fails immediately with no fallback (no blkdiscard, no shred). A frozen drive typically requires a power cycle to unfreeze; unsupported security means secure erase is not possible on that drive at all.

**SATA SSD erase-failure cleanup:** If the `--security-erase`/`--security-erase-enhanced` command fails *after* the user password has already been set, the drive would be left **LOCKED** on next power cycle (per kernel.org's ATA Secure Erase documentation) unless the password is disabled again. On erase failure, the agent makes a best-effort `hdparm --user-master u --security-disable p <dev>` call before returning. If the disable also fails, both errors are logged, but the action always fails with the **original erase error** — the disable outcome never masks it and never turns a failure into a success.

**No verification step:** Unlike `partition-disks`, `wipe-disks` does not attempt to verify erasure by reading data back — cryptographic and enhanced erase legitimately leave non-zero data on disk (only the encryption key is destroyed), so a data readback would produce false failures. Success is solely the erase command exiting 0.

**No timeout:** `shred` and ATA secure erase can run for many hours on large or slow disks. The agent runs these commands with no hard timeout and streams their stdout/stderr to the log as progress. (`kill_on_drop` is set on the child process purely as insurance against an orphaned process if the future is ever dropped before completion — it introduces no wall-clock or idle-output timeout.)

**Implementation:** `rack-agent/src/wipe.rs::wipe_disks()`

**System Dependencies:**
- `hdparm` - ATA Secure Erase (SATA SSDs)
- `nvme-cli` (`nvme` command) - NVMe format/crypto erase
- `coreutils` (`shred`) - HDD single-pass overwrite
- `lvm2` (`pvs`, `vgchange`, `vgremove`) - LVM/dm cleanup before erase

---

### 5. Daemon

**Purpose:** Continuously polls rack-director for actions and executes them without rebooting between actions

**CLI Usage:**
```bash
rack-agent --director-url http://rack-director:3000/cnc daemon
```

**Kernel Cmdline Usage:**
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=daemon
```

**What it does:**
1. Reads device UUID from SMBIOS once at startup (cached for the process lifetime)
2. Polls `GET /cnc/poll?uuid={UUID}` every 5 seconds when idle
3. Dispatches received actions to the appropriate handler (`discover_hardware`, `configure_bmc`, `partition_disks`)
4. Polls immediately after each completed action (no sleep)
5. On `reboot_device` or `install_os`: exits cleanly; systemd handles the reboot

**Implementation:** `rack-agent/src/daemon.rs::run_daemon()`

**⚠ PXE Boot Assumption:**
`install_os` and `reboot_device` cause the daemon to exit **without** calling `action_success`.
This is intentional — do not "fix" this by adding an `action_success` call.
Plan advancement for these actions is handled externally by rack-director's `on_boot()`:

- `reboot_device`: `advance_on_boot() == true` → `on_boot()` auto-advances the plan on the
  next PXE boot. Calling `action_success` here would cause a **double-advance**: once from
  `action_success`, then again from `on_boot()`.
- `install_os`: `advance_on_boot() == false` → `on_boot()` does NOT advance the plan; instead
  it serves the OS installer boot target. The OS installer calls `action_success` when the
  installation is complete. Calling `action_success` here would advance the plan **before** the
  OS is installed, causing the installer to never run.

**Known limitation:** if the machine crashes after the daemon exits but before rebooting, the
plan remains in `running` state indefinitely with no timeout. This requires manual intervention
to reset the plan.

**If daemon mode is ever run outside a PXE environment** (e.g. on a provisioned node), `install_os`
will loop forever because `on_boot()` is never called.

---

## Communication Protocol

### Rack Director Endpoints

The agent communicates with rack-director via the `/cnc/` endpoint namespace:

| Endpoint | Method | Purpose | Payload |
|----------|--------|---------|---------|
| `/cnc/update_attributes` | POST | Upload device attributes | `{uuid, attributes}` |
| `/cnc/action_success` | POST | Report action completion | `{uuid}` |
| `/cnc/action_failed` | POST | Report action failure | `{uuid, error_message}` |
| `/cnc/devices/{uuid}/bmc_config` | GET | Fetch BMC configuration | N/A |
| `/cnc/devices/{uuid}/disk_layout` | GET | Fetch disk layout | N/A |

### Communication Flow

```
┌──────────┐                          ┌────────────────┐
│  Agent   │                          │ rack-director  │
└────┬─────┘                          └────────┬───────┘
     │                                         │
     │ 1. Boot with action in cmdline          │
     │    rackdirector.action=daemon           │
     │                                         │
     │ 2. Resolve rack-director URL            │
     │    (from cmdline or --director-url)     │
     │                                         │
     │ 3. Execute action                       │
     │                                         │
     │ 4. Fetch config (if needed)             │
     │    GET /cnc/devices/{uuid}/...          │
     ├────────────────────────────────────────►│
     │                                         │
     │ 5. Perform action                       │
     │    (scan, configure, partition)         │
     │                                         │
     │ 6. Report status                        │
     │    POST /cnc/action_success             │
     │    or POST /cnc/action_failed           │
     ├────────────────────────────────────────►│
     │                                         │
     │                                         │ Update device
     │                                         │ and plan state
     │                                         │
     │ 7. Exit (plan advances, reboot)         │
     └─────────────────────────────────────────┘
```

### Error Handling

**Success Path:**
1. Action completes successfully
2. Agent calls `/cnc/action_success` with device UUID
3. rack-director advances plan to next action
4. Agent exits with code 0

**Failure Path:**
1. Action fails (validation, execution error, etc.)
2. Agent calls `/cnc/action_failed` with UUID and error message
3. rack-director marks plan as failed
4. Agent exits with code 10
5. Device remains in current lifecycle state

**Network Failures:**
- If agent can't reach rack-director, action fails
- Agent retries may be implemented (future enhancement)
- Device will retry on next boot (if plan still active)

---

## Network Interface Scanning

The agent scans physical Ethernet network interfaces from `/sys/class/net/`.

**What is scanned:**
- Interface name (e.g., eth0, eno1, enp0s3)
- MAC address from `/sys/class/net/{interface}/address`
- Link speed from `/sys/class/net/{interface}/speed` (in Mbps)
- IP address backfilled by rack-director from DHCP leases

**Filtering:**
- Loopback interfaces (lo) are excluded
- Virtual interfaces (no `/sys/class/net/{interface}/device/`) are excluded
- Non-Ethernet interfaces (type != 1) are excluded

**Speed Detection:**
- Read from `/sys/class/net/{interface}/speed`
- Returns `None` if:
  - Speed file doesn't exist
  - Link is down (speed = -1)
  - Speed cannot be parsed
- Returns actual speed in Mbps (e.g., 100, 1000, 10000) for active links

**Implementation:** `rack-agent/src/scan.rs::scan_network_interfaces()`

---

## Device UUID Resolution

The agent identifies devices using UUID from SMBIOS/DMI tables.

**Resolution Order:**
1. Try `/sys/firmware/dmi/tables/smbios_entry_point`
2. Fall back to `/sys/firmware/dmi/tables/DMI`
3. Extract UUID from DMI Structure Type 1 (System Information)
4. UUID format: `550e8400-e29b-41d4-a716-446655440000`

**Implementation:**
```rust
// Used by all commands
async fn get_device_uuid() -> Result<String>
```

**Location:** Multiple locations (scan.rs, partition.rs)

**Error:** If UUID cannot be read, action fails with error message

---

## Kernel Cmdline Parsing

The agent supports two configuration methods:

### 1. Command-Line Flags (Development/Testing)
```bash
rack-agent --director-url http://localhost:3000/cnc --action device-scan
```

### 2. Kernel Cmdline (Production/PXE Boot)
```
rackdirector.url=http://rack-director:3000/cnc rackdirector.action=daemon
```

rack-director always embeds `rackdirector.action=daemon` for agent image boots. The individual action names (`device-scan`, `configure-bmc`, `partition-disks`) are still accepted by the agent for direct/manual invocation.

**Parsing Logic:**
1. Check for `--director-url` flag, else parse `/proc/cmdline` for `rackdirector.url=`
2. Check for `--action` flag, else parse `/proc/cmdline` for `rackdirector.action=`
3. Default action: `device-scan` (if neither provided)

**Implementation:** `rack-agent/src/main.rs::resolve_director_url()`, `resolve_action()`

---

## Extending the Agent

### Adding a New Command

To add a new agent command (e.g., `ConfigureNetwork`):

#### 1. Add Command Variant

**File:** `rack-agent/src/main.rs`

```rust
#[derive(Subcommand, Debug)]
enum Command {
    DeviceScan(scan::DeviceScanArgs),
    ConfigureBmc,
    PartitionDisks,
    ConfigureNetwork,  // NEW
}
```

#### 2. Create Implementation Module

**File:** `rack-agent/src/network.rs` (new file)

```rust
use anyhow::Result;
use crate::client::RackDirector;

pub async fn configure_network(client: &RackDirector) -> Result<()> {
    // 1. Get device UUID
    let uuid = get_device_uuid().await?;

    // 2. Fetch network config from rack-director
    let config = client.get_network_config(&uuid).await?;

    // 3. Apply network configuration
    apply_network_config(&config).await?;

    // 4. Report success
    client.action_success(&uuid).await?;

    Ok(())
}
```

#### 3. Add Client Method (if fetching config)

**File:** `rack-agent/src/client.rs`

```rust
pub async fn get_network_config(&self, uuid: &str) -> Result<NetworkConfig> {
    let response = self
        .client
        .get(format!("{}/devices/{}/network_config", self.url, uuid))
        .send()
        .await?;

    let config = response.json::<NetworkConfig>().await?;
    Ok(config)
}
```

#### 4. Add Command Handler

**File:** `rack-agent/src/main.rs`

```rust
let result = if let Some(command) = args.command {
    match command {
        Command::DeviceScan(device_args) => scan::device_scan(&client, &device_args).await,
        Command::ConfigureBmc => scan::bmc_configure(&client).await,
        Command::PartitionDisks => partition::partition_disks(&client).await,
        Command::ConfigureNetwork => network::configure_network(&client).await,  // NEW
    }
} else {
    // Also add to action string matching
    match action.as_str() {
        "device-scan" => { /* ... */ }
        "configure-bmc" => { /* ... */ }
        "partition-disks" => { /* ... */ }
        "configure-network" => network::configure_network(&client).await,  // NEW
        _ => { /* ... */ }
    }
}
```

#### 5. Add Rack-Director Endpoint

**File:** `rack-director/src/http/cnc/mod.rs`

```rust
async fn get_network_config(
    State(state): State<AppState>,
    Path(uuid): Path<String>,
) -> Result<Json<NetworkConfig>, Error> {
    // Fetch network config from database/role
    // Return JSON
}
```

Add route:
```rust
Router::new()
    .route("/devices/:uuid/network_config", get(get_network_config))  // NEW
```

#### 6. Test

```bash
# Build agent
cd rack-agent
cargo build

# Test locally (if rack-director running)
sudo ./target/debug/rack-agent --director-url http://localhost:3000/cnc configure-network

# Test with rack-simulator (for full PXE boot flow)
# See @.claude/docs/actions-reference.md for rack-simulator usage
```

---

## Building the Agent

### Development Build

```bash
cd rack-agent
cargo build
```

**Binary:** `target/debug/rack-agent`

### Release Build

```bash
cd rack-agent
cargo build --release
```

**Binary:** `target/release/rack-agent`

### Building Agent Boot Image

The agent is packaged into a bootable initramfs image for PXE boot.

**Build Process:**
1. Build agent binary (release mode)
2. Create initramfs with agent and dependencies
3. Package kernel and initramfs for TFTP serving

**Build Location:** `agent-image/` directory

**See Also:** `agent-image/` documentation (to be created)

---

## Testing

### Unit Tests

```bash
cd rack-agent
cargo test
```

**Test Coverage:**
- URL resolution from cmdline
- Action resolution from cmdline
- DeviceScanArgs creation
- (More tests needed for scan and partition modules)

### Integration Testing with rack-simulator

Use `rack-simulator` to simulate device behavior:

1. Start rack-director
2. Start rack-simulator
3. Configure rack-simulator to make DHCP/TFTP requests
4. Verify agent boots and executes actions
5. Check rack-director for uploaded device data

### End-to-End Testing with QEMU

The `rack-simulator e2e` commands run the agent in a real QEMU VM against virtual disks for full provisioning validation:

```bash
# Run a single e2e test
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml

# Run all tests in parallel
cargo run --bin rack-simulator -- e2e run-all e2e-tests/ --parallel
```

**See:** @.claude/docs/e2e-testing.md for full e2e testing documentation

### Manual Testing

**Test device-scan locally:**
```bash
# Requires root for SMBIOS access
sudo ./target/debug/rack-agent --director-url http://localhost:3000/cnc device-scan --no-upload
```

**Test partition-disks (DANGER: will wipe disks!):**
```bash
# Only test on non-production systems or VMs
sudo ./target/debug/rack-agent --director-url http://localhost:3000/cnc partition-disks
```

---

## Common Issues

### "Failed to read device UUID from SMBIOS"

**Cause:** Agent can't access SMBIOS tables

**Solutions:**
- Run with root/sudo
- Check `/sys/firmware/dmi/tables/` exists
- Verify running on physical hardware or VM with SMBIOS support

### "Failed to resolve director URL"

**Cause:** URL not provided in cmdline or flag

**Solutions:**
- Provide `--director-url` flag
- Add `rackdirector.url=` to kernel cmdline
- Check `/proc/cmdline` content

### "Failed to fetch disk layout: 404"

**Cause:** Device not registered or role not assigned

**Solutions:**
- Ensure device ran `device-scan` first
- Verify device has assigned role in rack-director UI
- Check role has disk_layout configured

### "Device not found: /dev/disk/by-path/..."

**Cause:** Disk path in layout doesn't exist on device

**Solutions:**
- Run `lsblk` to see available disks
- Update disk layout to use correct device names
- Use `/dev/disk/by-path/` or `/dev/disk/by-id/` for consistency

---

## Agent Boot Flow

### PXE Boot Sequence

```
1. Device powers on
   │
   ├─► 2. PXE ROM requests DHCP
   │
   ├─► 3. Rack-director DHCP responds with:
   │      - IP address
   │      - Next-server (TFTP server)
   │      - Boot filename
   │
   ├─► 4. Device requests kernel and initramfs via TFTP
   │      - vmlinuz (kernel)
   │      - initramfs.cpio.gz (contains rack-agent)
   │
   ├─► 5. Device boots kernel with cmdline:
   │      rackdirector.url=http://.../cnc
   │      rackdirector.action=daemon
   │
   ├─► 6. Init process launches rack-agent in daemon mode
   │
   ├─► 7. Daemon polls GET /cnc/poll, executes actions
   │      - No reboot between actions
   │      - Exits only for reboot_device or install_os
   │
   └─► 8. On exit, system reboots (next PXE boot resumes daemon)
```

### Initramfs Layout

```
/
├── bin/
│   ├── rack-agent          # The agent binary
│   └── busybox             # Shell and utilities
├── lib/                    # Shared libraries
├── etc/
│   └── init                # Init script (launches rack-agent)
└── dev/                    # Device nodes
```

**Init Script:**
The init script reads kernel cmdline, sets up environment, and launches rack-agent with appropriate arguments.

---

## Dependencies

### Rust Crates

**Key dependencies:**
- `anyhow` - Error handling
- `clap` - Command-line argument parsing
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `serde`/`serde_json` - JSON serialization
- `log` - Logging
- `std_logger` - Logfmt formatter
- `dmidecode` - SMBIOS parsing

**See:** `rack-agent/Cargo.toml` for full list

### System Dependencies

**Required during agent execution:**
- Linux kernel with `/proc/cmdline` and `/sys/firmware/dmi/`
- Network connectivity to rack-director
- `lsblk` command (for disk scanning)
- Partition tools: `parted`, `mkfs.*` commands

**Note:** These are bundled in the agent boot image (initramfs)

---

## See Also

- @rack-director/src/plans/actions/CLAUDE.md - Complete action documentation
- @rack-director/CLAUDE.md - rack-director service documentation
- @rack-simulator/CLAUDE.md - rack-simulator documentation
