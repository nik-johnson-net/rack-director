# CNC Reference

This document describes the CNC endpoints available.

## Install Script Template Variables

Install scripts are Handlebars templates served to the OS installer via `GET /cnc/install_script?uuid={uuid}`. The following variables are available:

### Device Information

| Variable | Description |
|---|---|
| `{{ device.uuid }}` | Device UUID |
| `{{ device.hostname }}` | Device hostname |
| `{{ device.mac_address }}` | Primary NIC MAC address |
| `{{ device.ip_address }}` | Primary NIC IP address |
| `{{ device.gateway }}` | Primary NIC gateway (empty string when its network has no gateway) |
| `{{ device.dns_servers }}` | Comma-separated DNS servers for the primary NIC |
| `{{ device.netmask }}` | Primary NIC netmask |
| `{{ device.prefix_length }}` | Primary NIC network prefix length (e.g. `24` for a /24 subnet) |

The single `device.*` network fields above describe the **primary** NIC — the one with a DHCP lease, or
(when no lease exists) the first NIC that has been assigned a network and IP. They are retained for
backward compatibility. For multi-NIC configuration use `{{ device.network_interfaces }}` below.

#### `{{ device.network_interfaces }}` — per-NIC network configuration

Iterate with `{{#each device.network_interfaces}}`. Contains one entry for **every** NIC that has an
**effective network** and an **effective IP**, where:

- **effective network** = the operator-assigned network (`assigned_network_id`) if set, otherwise the
  DHCP-derived `network_id`
- **effective IP** = the operator-assigned static IP (`assigned_ip`) if set, otherwise the DHCP-derived
  `ip_address`

This lets a NIC be configured with a static IP **even on a segment with no DHCP**. Each NIC applies its
**own** network's gateway and DNS (per-NIC gateway). Each item has:

| Field | Description |
|---|---|
| `{{ this.interface_name }}` | Interface name (e.g. `eth0`) |
| `{{ this.mac_address }}` | NIC MAC address |
| `{{ this.ip_address }}` | Effective IP for this NIC |
| `{{ this.netmask }}` | Netmask of this NIC's network |
| `{{ this.prefix_length }}` | Prefix length of this NIC's network |
| `{{ this.gateway }}` | This NIC's network gateway, or **absent** when the network has no gateway — guard with `{{#if this.gateway}}` |
| `{{ this.dns_servers }}` | Comma-separated DNS servers, **empty** when the network has none — guard with `{{#if this.dns_servers}}` |
| `{{ this.is_primary }}` | `true` for the primary NIC (see above) |

A network may be defined with **no gateway and/or no DNS**, in which case those attributes are not set on
NICs attached to it (no default route / no nameserver). Always guard `gateway`/`dns_servers` with
`{{#if}}` in templates.

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
# Network — one directive per assigned NIC (gateway/DNS omitted when the network has none)
{{#each device.network_interfaces}}
network --bootproto=static --device={{this.mac_address}} --ip={{this.ip_address}} --netmask={{this.netmask}} {{#if this.gateway}}--gateway={{this.gateway}} {{/if}}{{#if this.dns_servers}}--nameserver={{this.dns_servers}} {{/if}}--activate{{#if this.is_primary}} --hostname={{../device.hostname}}{{/if}}
{{/each}}

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
{{#each device.network_interfaces}}
        {{this.interface_name}}:
          match:
            macaddress: "{{this.mac_address}}"
          addresses: ["{{this.ip_address}}/{{this.prefix_length}}"]
{{#if this.gateway}}          routes:
            - to: default
              via: {{this.gateway}}
{{/if}}{{#if this.dns_servers}}          nameservers:
            addresses: [{{this.dns_servers}}]
{{/if}}{{/each}}
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

### `wipe_all_disks`

Optional boolean field on the disk layout (default `false`).

When `true`, the agent erases partition info (`wipefs --all --force` + `sgdisk --zap-all`) from **every** whole disk on the machine before partitioning the disks listed in the layout. This clears stale metadata from disks that are not part of the layout (e.g., leftover data disks from a previous role). Disks in the layout are re-wiped afterwards; this is idempotent.

```json
{
  "disks": [
    { "device": "ROOT", "partition_table": "gpt", "partitions": [ ... ] }
  ],
  "wipe_all_disks": true
}
```

When `false` (the default), only the disks listed in `disks` are wiped.

### Disk Label Resolution

Platform labels (`ROOT`, `DATA1`, `DATA2`, etc.) are resolved to actual device paths using a two-level override system before being sent to the agent:

1. **Device override wins:** If the device has an entry in `disk_label_overrides` for that label (e.g., `{"ROOT": "/dev/disk/by-path/pci-0000:04:00.0-nvme-1"}`), that path is used directly.

2. **Canonical position matching (fallback):** Platform disks and device disks are each sorted by `(disk_type priority, size_gb)`. The index of the platform disk whose label matches is used to pick the corresponding device disk by position. This is robust across devices with different PCIe bus topologies.

3. **Error** if the label is not found in the platform's disk list.

Note: `PlatformDisk` does **not** have a `path` field. The path used for provisioning always comes from the device's own disk scan (`DiskInfo.path`).

#### `LABEL_OVERRIDE_DROPPED` Warning

When the agent submits a hardware re-scan, existing device label overrides are validated against the incoming disk list. If a by-path stored in an override no longer appears in the new `disks` list (e.g., the disk was replaced or moved), the override is dropped and a `DeviceWarning` is created with code `LABEL_OVERRIDE_DROPPED`. The warning surfaces on the device detail page in the UI.
