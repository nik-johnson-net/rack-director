# CNC Reference

This document describes the CNC endpoints available.

## Install Script Template Variables

Install scripts are Handlebars templates served to the OS installer via `GET /cnc/install_script?uuid={uuid}`. The following variables are available:

### Device Information

| Variable | Description |
|---|---|
| `{{ device.uuid }}` | Device UUID |
| `{{ device.hostname }}` | Device hostname |
| `{{ device.mac_address }}` | Primary MAC address |
| `{{ device.ip_address }}` | IP address from DHCP lease |
| `{{ device.gateway }}` | Network gateway |
| `{{ device.dns_servers }}` | Space-separated DNS servers |
| `{{ device.netmask }}` | Network netmask |
| `{{ device.prefix_length }}` | Network prefix length (e.g. `24` for a /24 subnet) |

### Role & OS

| Variable | Description |
|---|---|
| `{{ role.name }}` | Role name |
| `{{ role.disk_layout }}` | Disk layout as raw JSON (unresolved) |
| `{{ os.name }}` | OS name |
| `{{ os.version }}` | OS version |
| `{{ config.* }}` | Any key from `role.config_template` JSON |

### Disk Layout (resolved, post-partitioning)

These variables are populated from the device's resolved disk layout — platform labels (e.g. `ROOT`, `DATA1`) are already resolved to actual device paths. They reflect the partitions created by the `partition_disks` action.

#### `{{ partitions }}` — list of all partitions

Iterate with `{{#each partitions}}`. Each item has:

| Field | Description |
|---|---|
| `{{ this.disk }}` | Disk device path (e.g. `/dev/disk/by-path/pci-0000:00:03.0-nvme-1`) |
| `{{ this.device }}` | Partition device path including `/dev/` prefix |
| `{{ this.device_name }}` | Partition path without `/dev/` prefix — use for Kickstart `--onpart=` |
| `{{ this.label }}` | GPT partition label (e.g. `"efi"`, `"root"`) |
| `{{ this.size }}` | Partition size string (e.g. `"512MiB"`, `"rest"`) |
| `{{ this.filesystem }}` | Filesystem type, `null` for LVM/ZFS partitions |
| `{{ this.mount_point }}` | Mount point, `null` if not directly mounted |
| `{{ this.flags }}` | Array of partition flags (e.g. `["esp"]`, `["lvm"]`) |
| `{{ this.volume_group }}` | LVM VG name for PV partitions, `null` for regular partitions |

#### `{{ logical_volumes }}` — list of LVM logical volumes

Iterate with `{{#each logical_volumes}}`. Each item has:

| Field | Description |
|---|---|
| `{{ this.device }}` | LV device path (e.g. `/dev/vg0/root`) |
| `{{ this.device_name }}` | LV path without `/dev/` prefix (e.g. `vg0/root`) |
| `{{ this.vg_name }}` | Volume group name |
| `{{ this.lv_name }}` | Logical volume name |
| `{{ this.size }}` | LV size string |
| `{{ this.filesystem }}` | Filesystem type |
| `{{ this.mount_point }}` | Mount point, `null` if not mounted |

### Example: Kickstart (RHEL/Anaconda)

```kickstart
# Network
network --bootproto=static --ip={{ device.ip_address }} --netmask={{ device.netmask }} --gateway={{ device.gateway }} --nameserver={{ device.dns_servers }} --hostname={{ device.hostname }}

# Partitions (pre-existing, created by partition_disks action)
{{#each partitions}}{{#if this.mount_point}}{{#unless this.volume_group}}
part {{this.mount_point}} --fstype="{{this.filesystem}}" --onpart={{this.device_name}}
{{/unless}}{{/if}}{{/each}}

# LVM logical volumes
{{#each logical_volumes}}{{#if this.mount_point}}
logvol {{this.mount_point}} --vgname={{this.vg_name}} --name={{this.lv_name}} --fstype={{this.filesystem}}
{{/if}}{{/each}}

# Packages from role config
%packages
{{#each config.packages}}
{{this}}
{{/each}}
%end
```

### Example: Ubuntu Autoinstall (cloud-init)

```yaml
autoinstall:
  version: 1
  identity:
    hostname: {{ device.hostname }}
  network:
    network:
      version: 2
      ethernets:
        primary:
          match:
            macaddress: "{{ device.mac_address }}"
          addresses: ["{{ device.ip_address }}/24"]
          gateway4: {{ device.gateway }}
          nameservers:
            addresses: [{{ device.dns_servers }}]
  storage:
    layout:
      name: direct
```

### Example: Debian Preseed

```
d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}
```

---

## Disk Layout Configuration

Disk layouts are configured at the role level and applied by the `partition_disks` action.

**Boot partitions are WYSIWYG — they must be explicitly stored in the role's `disk_layout`.** rack-director does NOT auto-inject `esp` or `bios_grub` partitions at runtime. The server validates that:
- `firmware_mode = uefi` → ROOT disk must contain an `esp`-flagged partition
- `firmware_mode = bios` + GPT → ROOT disk must contain a `bios_grub`-flagged partition
- `firmware_mode` unset + GPT → ROOT disk must contain at least one of `esp` or `bios_grub`

Supported partition flags: `esp`, `boot`, `lvm`, `bios_grub`.

- `esp` — EFI System Partition (UEFI boot)
- `boot` — marks the boot partition
- `lvm` — marks a Physical Volume for LVM
- `bios_grub` — GRUB BIOS boot partition on GPT disks (BIOS/legacy boot only, no filesystem needed)

```toml
# Simple GPT layout for UEFI with EFI and root partition
[disk_layout]
disks = [
  { device = "ROOT", partition_table = "gpt", partitions = [
    { label = "efi", size = "600MiB", filesystem = "vfat", mount_point = "/boot/efi", flags = ["esp"] },
    { label = "boot", size = "1GiB", filesystem = "ext4", mount_point = "/boot" },
    { label = "root", size = "rest", filesystem = "xfs", mount_point = "/" }
  ]}
]
```

```toml
# Simple GPT layout for BIOS/legacy boot
[disk_layout]
disks = [
  { device = "ROOT", partition_table = "gpt", partitions = [
    { label = "biosboot", size = "2MiB", flags = ["bios_grub"] },
    { label = "boot", size = "1GiB", filesystem = "ext4", mount_point = "/boot", flags = ["boot"] },
    { label = "root", size = "rest", filesystem = "xfs", mount_point = "/" }
  ]}
]
```

```toml
# LVM layout (UEFI — note: esp partition is required)
[disk_layout]
disks = [
  { device = "ROOT", partition_table = "gpt", partitions = [
    { label = "efi", size = "300MiB", filesystem = "vfat", mount_point = "/boot/efi", flags = ["esp"] },
    { label = "boot", size = "1GiB", filesystem = "ext4", mount_point = "/boot" },
    { label = "lvm", size = "rest", volume_group = "vg0", flags = ["lvm"] }
  ]}
]

[[disk_layout.volume_groups]]
name = "vg0"
logical_volumes = [
  { name = "root", size = "50G", filesystem = "ext4", mount_point = "/" },
  { name = "swap", size = "8G", filesystem = "swap" },
  { name = "home", size = "100%FREE", filesystem = "ext4", mount_point = "/home" }
]
```

### Disk Label Resolution

Platform labels (`ROOT`, `DATA1`, `DATA2`, etc.) are resolved to actual device paths using a two-level override system before being sent to the agent:

1. **Device override wins:** If the device has an entry in `disk_label_overrides` for that label (e.g., `{"ROOT": "/dev/disk/by-path/pci-0000:04:00.0-nvme-1"}`), that path is used directly.

2. **Canonical position matching (fallback):** Platform disks and device disks are each sorted by `(disk_type priority, size_gb)`. The index of the platform disk whose label matches is used to pick the corresponding device disk by position. This is robust across devices with different PCIe bus topologies.

3. **Error** if the label is not found in the platform's disk list.

Note: `PlatformDisk` does **not** have a `path` field. The path used for provisioning always comes from the device's own disk scan (`DiskInfo.path`).

#### `LABEL_OVERRIDE_DROPPED` Warning

When the agent submits a hardware re-scan, existing device label overrides are validated against the incoming disk list. If a by-path stored in an override no longer appears in the new `disks` list (e.g., the disk was replaced or moved), the override is dropped and a `DeviceWarning` is created with code `LABEL_OVERRIDE_DROPPED`. The warning surfaces on the device detail page in the UI.
