# Rack Director Actions Reference

## Overview

Actions are atomic operations that devices execute during lifecycle transitions. Each action represents a specific task like partitioning disks, installing an OS, or configuring hardware. Actions are organized into **Plans**, which are sequences of actions that move devices between lifecycle states.

## Lifecycle States and Transitions

### Device Lifecycle States

| State | Description |
|-------|-------------|
| **new** | Device created in rack-director but not yet seen on network |
| **unprovisioned** | Device discovered and ready for provisioning (hardware scanned, BMC configured) |
| **provisioned** | Device provisioned with OS and ready for use |
| **removed** | Device decommissioned, history preserved but no further actions allowed |
| **broken** | Device failed during transition, requires manual intervention |

### Lifecycle Transitions

```
┌─────────────┐
│     new     │
└──────┬──────┘
       │ discover
       ▼
┌─────────────────┐
│ unprovisioned   │◄──────────┐
└────────┬────────┘           │
         │ provision          │ deprovision
         ▼                    │
    ┌─────────────┐           │
    │ provisioned │───────────┘
    └─────────────┘

    Any state → broken (on failure)
    broken → unprovisioned (repair)
    unprovisioned → removed (decommission)
```

### Transition Types and Their Plans

| Transition Type | From State | To State | Actions |
|-----------------|------------|----------|---------|
| **Discover** | new | unprovisioned | discover_hardware, configure_bmc |
| **Provision** | unprovisioned | provisioned | partition_disks, install_os |
| **Deprovision** | provisioned | unprovisioned | backup_data, remove_software, factory_reset |
| **Remove** | unprovisioned | removed | secure_wipe, inventory_removal |
| **Repair** | broken | unprovisioned | run_diagnostics, repair_issues, verify_functionality |

## Action Details

See `rack-director/src/plans/actions/CLAUDE.md` for individual action documentation, the agent communication protocol, and instructions for creating custom actions.

---

## Configuration Hierarchies

### Kernel Command Line Arguments

Kernel cmdline arguments use a **3-tier merging system** with override precedence:

1. **OS Cmdline (Base):** Default cmdline from Operating System installer configuration
2. **Role Cmdline (Override):** Role-specific cmdline arguments
3. **Device Cmdline (Final Override):** Device-specific cmdline arguments

**Merging Behavior:**
- Arguments are space-separated
- Later tiers override earlier tiers when arguments conflict
- Non-conflicting arguments are merged together

**Example:**
```
OS:     "console=ttyS0,115200 quiet splash"
Role:   "ip=dhcp hostname=worker1"
Device: "debug loglevel=7"

Final:  "console=ttyS0,115200 quiet splash ip=dhcp hostname=worker1 debug loglevel=7"
```

**When to Use Each Level:**

| Level | Use For | Example |
|-------|---------|---------|
| **OS** | Universal installer settings | `"console=ttyS0,115200"` |
| **Role** | Role-specific configuration | `"ip=dhcp"` for workers, `"ip=static"` for controllers |
| **Device** | Device-specific overrides | `"debug loglevel=7"` for debugging specific device |

**Implementation:** `rack-director/src/templates/mod.rs::merge_cmdline_args()`

**Database Migration:** Added in migration v10 (commit 7c6b810)

---

### Disk Layouts

Disk partition layouts are **defined at the Role level** and applied during the `partition_disks` action. Supports simple partitions, LVM volume groups, and ZFS pools.

**Storage:** `roles.disk_layout` column (JSON)

**Per-Device Overrides:** Not yet supported (future enhancement)

**Configuration Location:** Role Edit page in rack-director-ui

**Platform Labels:** Layouts can reference platform labels (e.g., `"ROOT"`, `"DATA1"`) instead of device paths. Labels are resolved to actual device paths at provisioning time based on the device's platform. This allows a single role to work across different hardware.

**Role Assignment Validation:**
When assigning a role to a device, rack-director validates:
- If disk layout uses labels: device must have a platform assigned
- All labels in the layout must exist in the device's platform
- Roles with only device paths (no labels) can be assigned without a platform

**Validation:**
- Sizes must be valid (fixed, percentage, or "rest")
- Only one "rest" size per device
- Filesystems must be supported
- Partition flags must be valid
- Platform labels must exist in the device's platform (validated at role assignment)

**Best Practices:**
- Prefer platform labels (`"ROOT"`, `"DATA1"`) over device paths for portability
- Use device paths (`/dev/disk/by-path/...`) only for fixed hardware configurations — prefer platform labels for portability
- Define EFI partition first (512MiB-1GiB, vfat, flags: ["esp", "boot"])
- Use "rest" for the last partition on each device
- Consider swap size based on RAM (16GiB+ for systems with <32GiB RAM)

---

## Multi-Stage Provisioning Workflow

Modern provisioning (unprovisioned → provisioned) is a **multi-stage process**:

```
┌─────────────────────┐
│  unprovisioned      │
└──────────┬──────────┘
           │
           ▼
    ┌──────────────────────────┐
    │  Stage 1: partition_disks │ ◄─── NetBoot rack-agent (daemon mode)
    └──────────┬───────────────┘      rackdirector.action=daemon
               │
               ▼
    ┌──────────────────────┐
    │  Stage 2: install_os  │ ◄─── NetBoot OS installer
    └──────────┬───────────┘      merged cmdline
               │
               ▼
       ┌─────────────┐
       │ provisioned │
       └─────────────┘
```

**Stage Details:**

1. **partition_disks:**
   - Device PXE boots with rack-agent image
   - Agent fetches disk layout from rack-director
   - Agent partitions and formats disks
   - Agent reports success
   - Plan advances to next action

2. **install_os:**
   - Device reboots and PXE boots with OS installer
   - Installer image (kernel + initramfs) loaded with merged cmdline
   - OS installed to pre-partitioned disks
   - Device configured and rebooted to local disk
   - Device reports success, plan completes

**Boot Target Switching:**
Each stage may use a different boot target (NetBoot configuration). Rack-director dynamically determines the correct boot target based on the device's current plan and action.

**Implementation:** `rack-director/src/director/mod.rs::get_boot_target()`

---

## Troubleshooting

### Action Failed

When an action fails:
1. Device reports failure via `/cnc/action_failed` with error message
2. Plan status set to `Failed`
3. Device transition marked as failed with error message
4. Device remains in current lifecycle state (does NOT advance)
5. Manual intervention required to debug and restart

**Check:**
- Device logs (if accessible via BMC or network console)
- Lifecycle transition history in rack-director (includes error messages)
- Plan execution status and current step

### Action Stuck

If an action appears stuck:
1. Check device network connectivity
2. Verify device is still PXE booting (check DHCP/TFTP logs)
3. Check rack-director logs for communication from device
4. Verify plan status (Pending vs Running)

**Common Causes:**
- Network connectivity issues
- DHCP/TFTP server problems
- Agent crash or boot failure
- Incorrect kernel cmdline (missing rackdirector.url or rackdirector.action)

### Partition Disk Layout Issues

Common partition_disks failures:

**"Device not found":**
- Device path doesn't exist on hardware
- If using platform labels: ensure device has a platform assigned and labels match
- Run device_scan to see available disks

**"Cannot assign role: disk layout uses platform labels but device has no platform":**
- Device needs a platform assigned before assigning a role that uses labels
- Run device_scan to trigger platform auto-detection, or assign manually

**"Label 'X' not found in platform":**
- The disk layout references a label not in the device's platform
- Check platform's disk labels in the UI
- Update the disk layout or create a platform with the required labels

**"Invalid size format":**
- Supported: `"512MiB"`, `"100GiB"`, `"50G"`, `"500GB"`, `"50%"`, `"rest"`
- Only one "rest" per device allowed

**"Filesystem not supported":**
- Verify filesystem is in supported list (ext4, ext3, ext2, xfs, btrfs, vfat, swap)

**"Partition flag invalid":**
- Check flags: boot, esp, lvm only

**LVM issues:**
- Ensure partitions with `volume_group` field have `"lvm"` flag set
- Ensure `volume_groups` array references existing partition volume_group names
- Last LV can use `"100%FREE"` or `"rest"` for remaining space

**ZFS issues:**
- ZFS tools must be available in the agent image
- Pool device references must match `"<disk_label>-<partition_label>"` format
- Pool vdev_type must be one of: `"single"`, `"mirror"`, `"raidz"`, `"raidz2"`
