# Actions Reference

Actions are atomic operations that devices execute during lifecycle transitions. They are organized into **Plans**, which are sequences of actions that move devices between lifecycle states.

**Implementation:** `rack-director/src/plans/actions/mod.rs`

**See also:** `rack-director/src/lifecycle/CLAUDE.md` for lifecycle states, transitions, and provisioning workflow.

---

## Action Details

### discover_hardware

**Purpose:** Scans device hardware and uploads metadata to rack-director

**Lifecycle:** Discover transition (new → unprovisioned)

**Agent Command:** `device-scan`

**What it does:**
- Scans CPU, memory, network interfaces, disk information
- Reads SMBIOS/DMI data for device UUID and hardware details
- Uploads device attributes to rack-director via `/cnc/update_attributes`
- **Triggers platform auto-detection**: After successful hardware scan, rack-director automatically detects or creates a matching Platform for the device based on hardware attributes

**Implementation:** `rack-agent/src/scan.rs::device_scan()`

**Platform Auto-Detection:**
After hardware discovery completes, rack-director:
1. Extracts hardware attributes (CPU, memory, disks, NICs)
2. Searches for matching platform (with tolerances: ±5% disk size, ±1 GiB memory)
3. If match found: assigns device to existing platform
4. If no match: creates new platform with auto-generated name and labels
5. Auto-assigns labels: ROOT = smallest+fastest disk, DATA1/DATA2 by bus order, NIC1/NIC2 by bus order

See @.claude/docs/platforms.md for detailed platform documentation.

**Configuration:** None

**Boot Target:** NetBoot with rack-agent image (daemon mode)
```
ramdisk: /cnc/agent-images/initramfs.cpio.gz
kernel: /cnc/agent-images/vmlinuz
cmdline: rackdirector.url=http://<server>/cnc rackdirector.action=daemon
```

---

### configure_bmc

**Purpose:** Configures BMC (Baseboard Management Controller) with static IP and credentials

**Lifecycle:** Discover transition (new → unprovisioned)

**Agent Command:** `configure-bmc`

**What it does:**
- Fetches BMC configuration from rack-director via `/cnc/devices/{uuid}/bmc_config`
- Configures BMC IP address (static or DHCP)
- Sets up BMC credentials (future: will create RACKDIRECTOR user)
- Reports success/failure to rack-director

**Implementation:** `rack-agent/src/scan.rs::bmc_configure()`

**Configuration:** Fetched from rack-director per-device
```json
{
  "ip_address_source": "static",
  "ip_address": "10.0.0.100",
  "netmask": "255.255.255.0",
  "gateway": "10.0.0.1",
  "username": "admin",
  "password": "secret"
}
```

**Boot Target:** NetBoot with rack-agent image (daemon mode)
```
cmdline: rackdirector.url=http://<server>/cnc rackdirector.action=daemon
```

---

### partition_disks

**Purpose:** Configures disk partitions, LVM volume groups, and ZFS pools based on Role disk layout configuration

**Lifecycle:** Provision transition (unprovisioned → provisioned)

**Agent Command:** `partition-disks`

**What it does:**
1. Gets device UUID from SMBIOS
2. Fetches resolved disk layout from rack-director via `/cnc/devices/{uuid}/disk_layout`
3. Applies layout in order:
   - Wipes and partitions each disk (wipefs, sgdisk, parted)
   - Waits for udev to settle
   - Sets up LVM volume groups and logical volumes (if configured)
   - Sets up ZFS pools and datasets (if configured)
   - Formats simple partitions (not LVM/ZFS)
4. Reports success/failure to rack-director

**Implementation:** `rack-agent/src/partition.rs::partition_disks()`

**Configuration:** Defined in Role.disk_layout (JSON), fetched from rack-director

**Platform Labels:** Disk layout can reference platform labels (e.g., `"ROOT"`, `"DATA1"`) instead of device paths. Labels are resolved to actual device paths by rack-director before being sent to the agent. See @.claude/docs/platforms.md for label documentation.

**Role Assignment Validation:** When assigning a role to a device, rack-director validates that any platform labels used in the disk layout exist in the device's assigned platform. Roles using labels cannot be assigned to devices without a platform.

**Disk Layout Format:**
```json
{
  "disks": [
    {
      "device": "/dev/sda",
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
  ]
}
```

**Disk Layout Fields:**
- **disks** (required): Array of disk configurations
  - **device**: Device path (`"/dev/sda"`) or platform label (`"ROOT"`)
  - **partition_table**: `"gpt"` (default) or `"msdos"`
  - **partitions**: Array of partition configurations
    - **label**: GPT partition name
    - **size**: Fixed size, percentage, or `"rest"`
    - **filesystem** (optional): Filesystem type (omit for LVM/ZFS partitions)
    - **mount_point** (optional): Mount point path
    - **flags** (optional): Partition flags (`"boot"`, `"esp"`, `"lvm"`)
    - **volume_group** (optional): LVM volume group this partition joins
- **volume_groups** (optional): Array of LVM volume group configurations
  - **name**: Volume group name
  - **logical_volumes**: Array of logical volume configurations
    - **name**: Logical volume name
    - **size**: Size (`"50G"`, `"100%FREE"`, `"rest"`)
    - **filesystem**: Filesystem type
    - **mount_point** (optional): Mount point path
- **zfs_pools** (optional): Array of ZFS pool configurations
  - **name**: Pool name
  - **vdev_type**: `"single"`, `"mirror"`, `"raidz"`, `"raidz2"`
  - **devices**: Partition references or device paths
  - **datasets**: Array of dataset configurations
    - **name**: Dataset name (relative to pool)
    - **mount_point** (optional): Mount point path
    - **properties** (optional): ZFS properties (`compression`, `atime`, etc.)
    - **zvol_size** (optional): Creates zvol instead of dataset if set
  - **properties** (optional): Pool-level properties (`ashift`, etc.)

**Supported Size Formats:**
- **Binary:** `"512MiB"`, `"100GiB"`, `"1TiB"`
- **Decimal:** `"500MB"`, `"100GB"`, `"1TB"`
- **Shorthand:** `"50G"`, `"100M"` (same as binary)
- **Percentage:** `"50%"` of total disk space
- **Rest:** `"rest"` — all remaining space (only one per device)

**Supported Filesystems:** ext2, ext3, ext4, xfs, btrfs, vfat, swap

**Supported Partition Flags:** boot, esp, lvm

**Boot Target:** NetBoot with rack-agent image (daemon mode)
```
cmdline: rackdirector.url=http://<server>/cnc rackdirector.action=daemon
```

**Example Disk Layouts:** See `@.claude/docs/actions-reference.md`

---

### install_os

**Purpose:** Installs operating system via PXE boot

**Lifecycle:** Provision transition (unprovisioned → provisioned)

**Agent Command:** N/A (OS installer runs directly)

**What it does:**
- Device boots from network with OS installer image
- Installer kernel and initramfs are loaded
- Installer uses merged kernel cmdline arguments (OS + Role + Device)
- OS is installed to disk (post-partitioning)
- Device reboots to local disk on completion

**Implementation:** PXE boot configuration in `rack-director/src/director/mod.rs::get_boot_target()`

**Kernel Cmdline Merging (3-Tier):**
```
Final cmdline = OS cmdline + Role cmdline + Device cmdline
                (base)       (override)      (final override)
```

**Boot Target:** NetBoot with OS installer
```
ramdisk: <os-initramfs-url>
kernel: <os-kernel-url>
cmdline: <merged-cmdline>
```

---

### Planned Actions

| Action | Lifecycle | Purpose |
|--------|-----------|---------|
| `backup_data` | Deprovision | Back up user data before deprovisioning |
| `remove_software` | Deprovision | Remove installed software |
| `factory_reset` | Deprovision | Reset device to factory state |
| `secure_wipe` | Remove | Securely wipe all data |
| `inventory_removal` | Remove | Remove device from active inventory |
| `run_diagnostics` | Repair | Run hardware diagnostics on broken devices |
| `repair_issues` | Repair | Attempt to repair known issues |
| `verify_functionality` | Repair | Verify device functionality after repair |

---

## Action Execution Protocol

### Agent Communication Flow

```
┌──────────┐                 ┌────────────────┐
│  Device  │                 │ rack-director  │
└────┬─────┘                 └────────┬───────┘
     │                                │
     │ 1. DHCP Request                │
     ├───────────────────────────────►│
     │                                │
     │ 2. DHCP Response + Boot Info   │
     │◄───────────────────────────────┤
     │                                │
     │ 3. TFTP: Get kernel/initramfs  │
     ├───────────────────────────────►│
     │                                │
     │ 4. Boot rack-agent with action │
     │    (from kernel cmdline)       │
     │                                │
     │ 5. Fetch config (if needed)    │
     │    GET /cnc/devices/{uuid}/... │
     ├───────────────────────────────►│
     │                                │
     │ 6. Execute action              │
     │    (scan, partition, etc.)     │
     │                                │
     │ 7. Report status               │
     │    POST /cnc/action_success    │
     │    or POST /cnc/action_failed  │
     ├───────────────────────────────►│
     │                                │
     │ 8. Update device/plan state    │◄─┐
     │                                │  │ rack-director
     │                                │──┘ advances plan
     │                                │
     │ 9. Reboot (next action)        │
     └────────────────────────────────┘
```

### CNC Endpoints (Device Communication)

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/cnc/update_attributes` | POST | Upload device attributes (from device_scan) |
| `/cnc/action_success` | POST | Report action completion |
| `/cnc/action_failed` | POST | Report action failure |
| `/cnc/devices/{uuid}/bmc_config` | GET | Fetch BMC configuration |
| `/cnc/devices/{uuid}/disk_layout` | GET | Fetch disk layout configuration |
| `/cnc/poll` | GET | Poll for the next pending action (daemon mode) |
| `/cnc/agent-images/vmlinuz` | GET | Download rack-agent kernel |
| `/cnc/agent-images/initramfs.cpio.gz` | GET | Download rack-agent initramfs |

**Implementation:** `rack-director/src/http/cnc/mod.rs`, `rack-director/src/http/cnc/poll.rs`

---

### Daemon Mode (Alternative to PXE-per-action)

When rack-agent runs in daemon mode (`rackdirector.action=daemon`), it polls
`GET /cnc/poll?uuid={UUID}` instead of reading the action from the kernel cmdline.
The endpoint returns the current plan action as JSON, or `204 No Content` if no plan is active.

**Wire format:** `{"type":"action","payload":{"type":"discover_hardware"}}`

The outer `type` envelope is forward-compatible — future message types (e.g. `cancel`,
`config_update`) can be added without breaking existing agents that only handle `action`.

**`RebootDevice` cannot be the first action in any plan.** A daemon agent may already
be running on the device — receiving `RebootDevice` as the first poll result would cause
an unnecessary reboot before any real work is done. `create_plan()` in `plans/store.rs`
enforces this at the persistence boundary. `RebootDevice` is valid as an intermediate
step (e.g. between two actions that require a clean reboot).

**`PollAction`** mirrors the `Action` enum variants and is defined in both:
- `rack-director/src/http/cnc/poll.rs` — server side (serialization, `From<&Action>`)
- `rack-agent/src/client.rs` — agent side (deserialization, `ServerMessage` wrapper)

Adding a new `Action` variant in `plans/actions/mod.rs` causes a compile error in `poll.rs`
until `PollAction` is updated — the `From<&Action>` match is exhaustive by design.

---

## Creating Custom Actions

To add a new action:

1. **Define the action in lifecycle plan:**
   - Add to `get_plan_stub_for_transition()` in `rack-director/src/lifecycle/mod.rs`

2. **Implement agent command (if needed):**
   - Add new command variant to `Command` enum in `rack-agent/src/main.rs`
   - Implement action logic in new module (e.g., `rack-agent/src/new_action.rs`)
   - Add match arm in `main()` to handle the new command

3. **Add configuration endpoint (if needed):**
   - Implement GET endpoint in `rack-director/src/http/cnc/mod.rs`
   - Add corresponding method to `RackDirector` client in `rack-agent/src/client.rs`

4. **Update boot target logic:**
   - Modify `get_boot_target()` in `rack-director/src/director/mod.rs`

5. **Test:**
   - Test with rack-simulator for PXE boot flow
   - Verify plan execution and state transitions
