use anyhow::Result;
use common::disk_layout::{DiskLayout, partition_path};
use handlebars::Handlebars;
use serde_json::json;
use uuid::Uuid;

/// Network information for a device (primary NIC, for backward-compatible single-value template fields).
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub mac_address: String,
    pub ip_address: String,
    /// Gateway for the primary NIC's network. `None` when the network has no gateway configured.
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub netmask: String,
    pub prefix_length: u8,
}

/// Per-NIC network configuration context for install-script templates.
///
/// One entry is emitted for every NIC that has an effective network and IP.
/// The "effective" values give operator-assigned config precedence over DHCP-derived info.
///
/// Available in templates as `{{#each device.network_interfaces}}`:
/// - `{{ this.interface_name }}` — interface name (e.g. `eth0`)
/// - `{{ this.mac_address }}` — NIC MAC address
/// - `{{ this.ip_address }}` — effective IP
/// - `{{ this.netmask }}` — netmask of the NIC's network
/// - `{{ this.prefix_length }}` — prefix length of the NIC's network
/// - `{{ this.gateway }}` — network gateway; absent when the network has no gateway — guard with `{{#if this.gateway}}`
/// - `{{ this.dns_servers }}` — comma-separated DNS servers; empty when none — guard with `{{#if this.dns_servers}}`
/// - `{{ this.is_primary }}` — `true` for the primary NIC (DHCP-lease NIC or first configured)
#[derive(Debug, Clone, serde::Serialize)]
pub struct NicNetworkContext {
    pub interface_name: String,
    pub mac_address: String,
    pub ip_address: String,
    pub netmask: String,
    pub prefix_length: u8,
    /// `None` when the NIC's network has no gateway configured.
    pub gateway: Option<String>,
    /// Comma-separated DNS servers; empty string when the network has no DNS configured.
    pub dns_servers: String,
    /// `true` for the primary NIC (the DHCP-lease NIC if any, else the first configured NIC).
    pub is_primary: bool,
}

/// Device attributes for template rendering
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub uuid: Uuid,
    pub hostname: Option<String>,
    /// Firmware mode detected during device scan. None if not yet detected.
    pub boot_mode: Option<common::FirmwareMode>,
}

/// Context for a single partition in the install script template.
///
/// Available template variables within `{{ partitions }}` loop:
/// - `{{ this.disk }}` - Disk device path (e.g., /dev/disk/by-path/...)
/// - `{{ this.device }}` - Partition device path (e.g., /dev/disk/by-path/...-part1)
/// - `{{ this.device_name }}` - Partition path without /dev/ prefix (e.g., disk/by-path/...-part1)
/// - `{{ this.disk_name }}` - Disk path without /dev/ prefix (e.g., disk/by-path/...)
/// - `{{ this.label }}` - GPT partition label
/// - `{{ this.size }}` - Partition size string
/// - `{{ this.filesystem }}` - Filesystem type, or null for LVM/ZFS partitions
/// - `{{ this.mount_point }}` - Mount point, or null
/// - `{{ this.flags }}` - Array of partition flags (e.g., ["esp", "boot"])
/// - `{{ this.volume_group }}` - LVM volume group name, or null
/// - `{{ this.is_bios_grub }}` - true if the partition has the bios_grub flag
/// - `{{ this.is_esp }}` - true if the partition has the esp flag
#[derive(Debug, Clone, serde::Serialize)]
pub struct PartitionContext {
    pub disk: String,
    pub device: String,
    pub device_name: String,
    /// Disk path without the /dev/ prefix (e.g., "disk/by-path/pci-...").
    /// Useful for Anaconda/kickstart directives that expect a bare device name.
    pub disk_name: String,
    pub label: String,
    pub size: String,
    pub filesystem: Option<String>,
    pub mount_point: Option<String>,
    pub flags: Vec<String>,
    pub volume_group: Option<String>,
    /// True when `flags` contains `"bios_grub"`. Provided as a convenience
    /// boolean so templates do not need to iterate the flags array.
    pub is_bios_grub: bool,
    /// True when `flags` contains `"esp"`. Provided as a convenience boolean
    /// so templates do not need to iterate the flags array.
    pub is_esp: bool,
}

/// Context for an LVM logical volume in the install script template.
///
/// Available template variables within `{{ logical_volumes }}` loop:
/// - `{{ this.device }}` - LV device path (e.g., /dev/vg0/root)
/// - `{{ this.device_name }}` - LV path without /dev/ prefix (e.g., vg0/root)
/// - `{{ this.vg_name }}` - Volume group name
/// - `{{ this.lv_name }}` - Logical volume name
/// - `{{ this.size }}` - LV size string
/// - `{{ this.filesystem }}` - Filesystem type, or null for raw LVs (e.g. Ceph)
/// - `{{ this.mount_point }}` - Mount point, or null
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogicalVolumeContext {
    pub device: String,
    pub device_name: String,
    pub vg_name: String,
    pub lv_name: String,
    pub size: String,
    pub filesystem: Option<String>,
    pub mount_point: Option<String>,
}

/// Build partition and logical volume context lists from a resolved disk layout.
///
/// The disk layout must already have platform labels resolved to actual device paths
/// before calling this function. Returns a tuple of (partitions, logical_volumes).
pub fn build_disk_layout_context(
    layout: &DiskLayout,
) -> (Vec<PartitionContext>, Vec<LogicalVolumeContext>) {
    let mut partitions = Vec::new();
    let mut logical_volumes = Vec::new();

    for disk in &layout.disks {
        for (i, partition) in disk.partitions.iter().enumerate() {
            let device = partition_path(&disk.device, i + 1);
            let device_name = device.trim_start_matches("/dev/").to_string();
            let disk_name = disk.device.trim_start_matches("/dev/").to_string();
            let flags = partition.flags.clone().unwrap_or_default();
            let is_bios_grub = flags.contains(&"bios_grub".to_string());
            let is_esp = flags.contains(&"esp".to_string());
            partitions.push(PartitionContext {
                disk: disk.device.clone(),
                device,
                device_name,
                disk_name,
                label: partition.label.clone(),
                size: partition.size.clone(),
                filesystem: partition.filesystem.clone(),
                mount_point: partition.mount_point.clone(),
                flags,
                volume_group: partition.volume_group.clone(),
                is_bios_grub,
                is_esp,
            });
        }
    }

    if let Some(ref vgs) = layout.volume_groups {
        for vg in vgs {
            for lv in &vg.logical_volumes {
                let device = format!("/dev/{}/{}", vg.name, lv.name);
                let device_name = format!("{}/{}", vg.name, lv.name);
                logical_volumes.push(LogicalVolumeContext {
                    device,
                    device_name,
                    vg_name: vg.name.clone(),
                    lv_name: lv.name.clone(),
                    size: lv.size.clone(),
                    filesystem: lv.filesystem.clone(),
                    mount_point: lv.mount_point.clone(),
                });
            }
        }
    }

    (partitions, logical_volumes)
}

/// Render an install script template using OSM-resolved context.
///
/// Similar to `render_install_script` but takes OS name/version as strings
/// and template variables for the config context.
///
/// The `network` parameter describes the primary NIC and populates the legacy
/// single-value `device.*` fields for backward compatibility.  The `nics` slice
/// contains one entry per NIC with an effective network/IP and is exposed as
/// `device.network_interfaces` for multi-NIC templates.
///
/// For the legacy `device.gateway` field, an empty string is emitted when the
/// primary NIC's network has no gateway (so existing templates don't print "null").
#[allow(clippy::too_many_arguments)]
pub fn render_install_script_osm(
    template: &str,
    device: &DeviceInfo,
    role_name: &str,
    role_disk_layout: &DiskLayout,
    role_config_template: &Option<serde_json::Value>,
    os_name: &str,
    os_version: &str,
    network: &NetworkInfo,
    nics: &[NicNetworkContext],
    disk_layout: &DiskLayout,
    root_url: &str,
) -> Result<String> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(handlebars::no_escape);

    let (partitions, logical_volumes) = build_disk_layout_context(disk_layout);

    // Deduplicated list of VG names in insertion order, for templates that need
    // to declare each volume group once (e.g., Anaconda's `volgroup` directive).
    let mut volume_groups: Vec<String> = Vec::new();
    for lv in &logical_volumes {
        if !volume_groups.contains(&lv.vg_name) {
            volume_groups.push(lv.vg_name.clone());
        }
    }

    let boot_mode_str = device
        .boot_mode
        .map(|m| match m {
            common::FirmwareMode::Bios => "bios",
            common::FirmwareMode::Uefi => "uefi",
        })
        .unwrap_or("");
    let is_uefi = device.boot_mode == Some(common::FirmwareMode::Uefi);
    let is_bios = device.boot_mode == Some(common::FirmwareMode::Bios);

    // Legacy `device.gateway` emits an empty string when the network has no gateway so that
    // existing templates referencing `{{ device.gateway }}` don't render "null".
    let legacy_gateway = network.gateway.clone().unwrap_or_default();
    let dns_csv = network.dns_servers.join(",");

    let context = json!({
        "device": {
            "uuid": device.uuid.to_string(),
            "hostname": device.hostname.as_deref().unwrap_or("unknown"),
            "mac_address": network.mac_address,
            "ip_address": network.ip_address,
            "gateway": legacy_gateway,
            "dns_servers": dns_csv,
            "dns_servers_csv": dns_csv,
            "netmask": network.netmask,
            "prefix_length": network.prefix_length,
            "boot_mode": boot_mode_str,
            "is_uefi": is_uefi,
            "is_bios": is_bios,
            "network_interfaces": nics,
        },
        "role": {
            "name": role_name,
            "disk_layout": role_disk_layout,
        },
        "os": {
            "name": os_name,
            "version": os_version,
        },
        "config": role_config_template,
        "partitions": partitions,
        "logical_volumes": logical_volumes,
        "volume_groups": volume_groups,
        "rack_director_url": root_url,
    });

    Ok(handlebars.render_template(template, &context)?)
}

pub fn render_cmdline_args(
    template: &str,
    root_url: &str,
    device_uuid: Option<&Uuid>,
) -> Result<String> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(handlebars::no_escape);

    let install_script_url = match device_uuid {
        Some(uuid) => format!("{}/cnc/install_script?uuid={}", root_url, uuid),
        None => format!("{}/cnc/install_script", root_url),
    };

    let context = json!({
        "install_script_url": install_script_url,
    });

    Ok(handlebars.render_template(template, &context)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::DiskLayout;
    use common::disk_layout::{DiskConfig, LogicalVolume, PartitionConfig, VolumeGroup};
    use uuid::Uuid;

    fn make_device() -> DeviceInfo {
        DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: None,
        }
    }

    fn make_network() -> NetworkInfo {
        NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: Some("10.0.0.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            netmask: "255.255.255.0".to_string(),
            prefix_length: 24,
        }
    }

    fn make_nic(
        mac: &str,
        ip: &str,
        gateway: Option<&str>,
        dns: &str,
        primary: bool,
    ) -> NicNetworkContext {
        NicNetworkContext {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: ip.to_string(),
            netmask: "255.255.255.0".to_string(),
            prefix_length: 24,
            gateway: gateway.map(|s| s.to_string()),
            dns_servers: dns.to_string(),
            is_primary: primary,
        }
    }

    fn empty_disk_layout() -> DiskLayout {
        DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        }
    }

    // ========== build_disk_layout_context tests ==========

    #[test]
    fn test_build_disk_layout_context_empty() {
        let layout = empty_disk_layout();
        let (partitions, logical_volumes) = build_disk_layout_context(&layout);
        assert!(partitions.is_empty());
        assert!(logical_volumes.is_empty());
    }

    #[test]
    fn test_build_disk_layout_context_ata_partitions() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "efi".to_string(),
                        size: "512MiB".to_string(),
                        filesystem: Some("vfat".to_string()),
                        mount_point: Some("/boot/efi".to_string()),
                        flags: Some(vec!["esp".to_string(), "boot".to_string()]),
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: "rest".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/".to_string()),
                        flags: None,
                        volume_group: None,
                    },
                ],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, logical_volumes) = build_disk_layout_context(&layout);

        assert_eq!(partitions.len(), 2);
        assert!(logical_volumes.is_empty());

        assert_eq!(
            partitions[0].disk,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
        assert_eq!(
            partitions[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part1"
        );
        assert_eq!(
            partitions[0].device_name,
            "disk/by-path/pci-0000:00:1f.2-ata-1-part1"
        );
        assert_eq!(partitions[0].label, "efi");
        assert_eq!(partitions[0].size, "512MiB");
        assert_eq!(partitions[0].filesystem, Some("vfat".to_string()));
        assert_eq!(partitions[0].mount_point, Some("/boot/efi".to_string()));
        assert_eq!(partitions[0].flags, vec!["esp", "boot"]);
        assert!(partitions[0].volume_group.is_none());

        assert_eq!(
            partitions[1].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part2"
        );
        assert_eq!(
            partitions[1].device_name,
            "disk/by-path/pci-0000:00:1f.2-ata-1-part2"
        );
        assert_eq!(partitions[1].label, "root");
        assert_eq!(partitions[1].filesystem, Some("ext4".to_string()));
        assert_eq!(partitions[1].flags, Vec::<String>::new());
    }

    #[test]
    fn test_build_disk_layout_context_nvme_partitions() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/nvme0n1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, _) = build_disk_layout_context(&layout);

        assert_eq!(partitions[0].device, "/dev/nvme0n1p1");
        assert_eq!(partitions[0].device_name, "nvme0n1p1");
    }

    #[test]
    fn test_build_disk_layout_context_by_path_partitions() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, _) = build_disk_layout_context(&layout);

        assert_eq!(
            partitions[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part1"
        );
        // device_name strips the /dev/ prefix
        assert_eq!(
            partitions[0].device_name,
            "disk/by-path/pci-0000:00:1f.2-ata-1-part1"
        );
    }

    #[test]
    fn test_build_disk_layout_context_lvm() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    PartitionConfig {
                        label: "boot".to_string(),
                        size: "1GiB".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/boot".to_string()),
                        flags: Some(vec!["boot".to_string()]),
                        volume_group: None,
                    },
                    PartitionConfig {
                        label: "lvm".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: Some(vec!["lvm".to_string()]),
                        volume_group: Some("vg0".to_string()),
                    },
                ],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![
                    LogicalVolume {
                        name: "root".to_string(),
                        size: "50G".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/".to_string()),
                    },
                    LogicalVolume {
                        name: "swap".to_string(),
                        size: "8G".to_string(),
                        filesystem: Some("swap".to_string()),
                        mount_point: None,
                    },
                ],
            }]),
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, logical_volumes) = build_disk_layout_context(&layout);

        assert_eq!(partitions.len(), 2);
        assert_eq!(logical_volumes.len(), 2);

        // LVM partition has no filesystem, has volume_group
        assert_eq!(
            partitions[1].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part2"
        );
        assert_eq!(
            partitions[1].device_name,
            "disk/by-path/pci-0000:00:1f.2-ata-1-part2"
        );
        assert!(partitions[1].filesystem.is_none());
        assert_eq!(partitions[1].volume_group, Some("vg0".to_string()));

        // LV devices
        assert_eq!(logical_volumes[0].device, "/dev/vg0/root");
        assert_eq!(logical_volumes[0].device_name, "vg0/root");
        assert_eq!(logical_volumes[0].vg_name, "vg0");
        assert_eq!(logical_volumes[0].lv_name, "root");
        assert_eq!(logical_volumes[0].size, "50G");
        assert_eq!(logical_volumes[0].filesystem, Some("ext4".to_string()));
        assert_eq!(logical_volumes[0].mount_point, Some("/".to_string()));

        assert_eq!(logical_volumes[1].device, "/dev/vg0/swap");
        assert_eq!(logical_volumes[1].device_name, "vg0/swap");
        assert!(logical_volumes[1].mount_point.is_none());
    }

    #[test]
    fn test_build_disk_layout_context_device_name_strips_dev_prefix() {
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-2".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "data".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("xfs".to_string()),
                    mount_point: Some("/data".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, _) = build_disk_layout_context(&layout);

        assert_eq!(
            partitions[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-2-part1"
        );
        assert_eq!(
            partitions[0].device_name,
            "disk/by-path/pci-0000:00:1f.2-ata-2-part1"
        );
    }

    #[test]
    fn test_build_disk_layout_context_mixed_partitions_and_lvm() {
        // Two-disk layout: one simple disk, one LVM disk
        let layout = DiskLayout {
            disks: vec![
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "boot".to_string(),
                        size: "512MiB".to_string(),
                        filesystem: Some("vfat".to_string()),
                        mount_point: Some("/boot/efi".to_string()),
                        flags: Some(vec!["esp".to_string()]),
                        volume_group: None,
                    }],
                },
                DiskConfig {
                    device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-2".to_string(),
                    partition_table: "gpt".to_string(),
                    partitions: vec![PartitionConfig {
                        label: "lvm".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: Some(vec!["lvm".to_string()]),
                        volume_group: Some("vg0".to_string()),
                    }],
                },
            ],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![LogicalVolume {
                    name: "home".to_string(),
                    size: "100%FREE".to_string(),
                    filesystem: Some("xfs".to_string()),
                    mount_point: Some("/home".to_string()),
                }],
            }]),
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let (partitions, logical_volumes) = build_disk_layout_context(&layout);

        assert_eq!(partitions.len(), 2);
        assert_eq!(logical_volumes.len(), 1);

        assert_eq!(
            partitions[0].disk,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
        assert_eq!(
            partitions[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part1"
        );
        assert_eq!(
            partitions[1].disk,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-2"
        );
        assert_eq!(
            partitions[1].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-2-part1"
        );

        assert_eq!(logical_volumes[0].device, "/dev/vg0/home");
        assert_eq!(logical_volumes[0].device_name, "vg0/home");
    }

    #[test]
    fn test_render_install_script_osm() {
        let template = "hostname: {{ device.hostname }}\nos: {{ os.name }} {{ os.version }}";
        let result = render_install_script_osm(
            template,
            &make_device(),
            "test-role",
            &empty_disk_layout(),
            &None,
            "Ubuntu",
            "22.04",
            &make_network(),
            &[],
            &empty_disk_layout(),
            "http://127.0.0.1:3000",
        )
        .unwrap();
        assert_eq!(result, "hostname: server01\nos: Ubuntu 22.04");
    }

    /// Verifies that the legacy single-value `device.gateway` field emits an empty string
    /// when the primary NIC's network has no gateway (so templates don't render "null").
    #[test]
    fn test_render_legacy_gateway_none_emits_empty_string() {
        let template = "gateway={{ device.gateway }}";
        let mut network = make_network();
        network.gateway = None;
        let result = render_install_script_osm(
            template,
            &make_device(),
            "test-role",
            &empty_disk_layout(),
            &None,
            "Linux",
            "1.0",
            &network,
            &[],
            &empty_disk_layout(),
            "http://127.0.0.1:3000",
        )
        .unwrap();
        assert_eq!(result, "gateway=");
    }

    /// Verifies that `device.network_interfaces` is accessible in templates and that
    /// per-NIC gateway/DNS fields render correctly, including the case where a NIC
    /// has no gateway (the `{{#if this.gateway}}` guard prevents output).
    #[test]
    fn test_render_network_interfaces_loop() {
        let template = concat!(
            "{{#each device.network_interfaces}}",
            "{{this.mac_address}}:{{this.ip_address}}",
            "{{#if this.gateway}}:gw={{this.gateway}}{{/if}}",
            "{{#if this.dns_servers}}:dns={{this.dns_servers}}{{/if}}",
            "{{#if this.is_primary}}:PRIMARY{{/if}}",
            "\n",
            "{{/each}}"
        );

        let nics = vec![
            // Primary NIC with gateway and DNS
            make_nic(
                "aa:bb:cc:dd:ee:01",
                "10.0.0.100",
                Some("10.0.0.1"),
                "8.8.8.8",
                true,
            ),
            // Secondary NIC — different gateway
            NicNetworkContext {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: "192.168.1.50".to_string(),
                netmask: "255.255.255.0".to_string(),
                prefix_length: 24,
                gateway: Some("192.168.1.1".to_string()),
                dns_servers: String::new(),
                is_primary: false,
            },
            // Third NIC — no gateway, no DNS
            make_nic("aa:bb:cc:dd:ee:03", "172.16.0.10", None, "", false),
        ];

        let result = render_install_script_osm(
            template,
            &make_device(),
            "test-role",
            &empty_disk_layout(),
            &None,
            "Linux",
            "1.0",
            &make_network(),
            &nics,
            &empty_disk_layout(),
            "http://127.0.0.1:3000",
        )
        .unwrap();

        let lines: Vec<&str> = result.lines().collect();
        // Primary NIC has gateway and is_primary marker
        assert!(
            lines[0].contains("aa:bb:cc:dd:ee:01:10.0.0.100:gw=10.0.0.1:dns=8.8.8.8:PRIMARY"),
            "Primary NIC line unexpected: {:?}",
            lines[0]
        );
        // Secondary NIC has its own gateway, no DNS, no PRIMARY marker
        assert!(
            lines[1].contains("aa:bb:cc:dd:ee:02:192.168.1.50:gw=192.168.1.1"),
            "Secondary NIC line unexpected: {:?}",
            lines[1]
        );
        assert!(
            !lines[1].contains("PRIMARY"),
            "Secondary NIC must not have PRIMARY marker"
        );
        // Third NIC — no gateway, no DNS emitted
        assert!(
            lines[2].contains("aa:bb:cc:dd:ee:03:172.16.0.10"),
            "Third NIC line unexpected: {:?}",
            lines[2]
        );
        assert!(!lines[2].contains("gw="), "Third NIC must not emit gateway");
        assert!(!lines[2].contains("dns="), "Third NIC must not emit dns");
    }

    /// Verifies that the legacy single-value fields resolve to the primary NIC even
    /// when multiple NICs are present.
    #[test]
    fn test_render_legacy_fields_resolve_to_primary() {
        let template =
            "ip={{ device.ip_address }} gw={{ device.gateway }} mac={{ device.mac_address }}";
        let nics = vec![
            make_nic(
                "aa:bb:cc:dd:ee:01",
                "10.0.0.100",
                Some("10.0.0.1"),
                "8.8.8.8",
                true,
            ),
            make_nic(
                "aa:bb:cc:dd:ee:02",
                "192.168.1.50",
                Some("192.168.1.1"),
                "",
                false,
            ),
        ];
        let result = render_install_script_osm(
            template,
            &make_device(),
            "test-role",
            &empty_disk_layout(),
            &None,
            "Linux",
            "1.0",
            &make_network(),
            &nics,
            &empty_disk_layout(),
            "http://127.0.0.1:3000",
        )
        .unwrap();
        // Legacy fields must come from `network` (the primary NetworkInfo), not the nics slice
        assert_eq!(result, "ip=10.0.0.100 gw=10.0.0.1 mac=00:11:22:33:44:55");
    }

    /// Verifies that the CentOS 10 kickstart template renders correctly for a
    /// GPT + LVM + biosboot disk layout. This is the canonical layout used for
    /// BIOS-mode provisioning: one bios_grub partition, one /boot partition,
    /// and one LVM PV that backs a vg0 VG with root and swap LVs.
    ///
    /// NOTE: This test may fail if the kickstart template has not yet been
    /// updated to use the new context fields (disk_name, is_bios_grub, is_esp,
    /// volume_groups). The test is intentionally written against the final
    /// expected output so it will pass once the template is updated.
    #[test]
    fn test_centos10_kickstart_gpt_lvm_biosboot() {
        let template_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("default-osm/centos-10/centos-10-amd64.ks");
        let template = std::fs::read_to_string(&template_path).unwrap();

        let disk_device = "/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0".to_string();
        let disk_layout = DiskLayout {
            disks: vec![DiskConfig {
                device: disk_device.clone(),
                partition_table: "gpt".to_string(),
                partitions: vec![
                    // Partition 1: BIOS boot (no filesystem, no mount point)
                    PartitionConfig {
                        label: "biosboot".to_string(),
                        size: "1MiB".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: Some(vec!["bios_grub".to_string()]),
                        volume_group: None,
                    },
                    // Partition 2: /boot ext4
                    PartitionConfig {
                        label: "boot".to_string(),
                        size: "1GiB".to_string(),
                        filesystem: Some("ext4".to_string()),
                        mount_point: Some("/boot".to_string()),
                        flags: None,
                        volume_group: None,
                    },
                    // Partition 3: LVM PV (no filesystem, no mount point)
                    PartitionConfig {
                        label: "lvm".to_string(),
                        size: "rest".to_string(),
                        filesystem: None,
                        mount_point: None,
                        flags: Some(vec!["lvm".to_string()]),
                        volume_group: Some("vg0".to_string()),
                    },
                ],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![
                    LogicalVolume {
                        name: "root".to_string(),
                        size: "14G".to_string(),
                        filesystem: Some("xfs".to_string()),
                        mount_point: Some("/".to_string()),
                    },
                    LogicalVolume {
                        name: "swap".to_string(),
                        size: "2G".to_string(),
                        filesystem: Some("swap".to_string()),
                        mount_point: None,
                    },
                ],
            }]),
            zfs_pools: None,
            wipe_all_disks: false,
        };

        let config_template = Some(serde_json::json!({
            "rootpw": "$6$fakehash",
            "packages": ["vim", "curl"],
            "postinstall": "echo done",
        }));

        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: Some(common::FirmwareMode::Bios),
        };

        let primary_nic = make_nic(
            "00:11:22:33:44:55",
            "10.0.0.100",
            Some("10.0.0.1"),
            "8.8.8.8",
            true,
        );
        let result = render_install_script_osm(
            &template,
            &device,
            "centos10-role",
            &disk_layout,
            &config_template,
            "CentOS",
            "10",
            &make_network(),
            &[primary_nic],
            &disk_layout,
            "http://127.0.0.1:3000",
        )
        .unwrap();

        // Kickstart must instruct Anaconda to zero the MBR and restrict the
        // install to only the target disk.
        assert!(
            result.contains("zerombr"),
            "expected 'zerombr' in rendered output"
        );
        assert!(
            result.contains("ignoredisk --only-use="),
            "expected 'ignoredisk --only-use=' in rendered output"
        );

        // The biosboot partition must be declared so GRUB can write its core
        // image into it.
        assert!(
            result.contains("part biosboot --fstype=biosboot"),
            "expected biosboot part directive in rendered output"
        );

        // The LVM PV declaration uses Anaconda's 'part pv.' syntax.
        assert!(
            result.contains("part pv."),
            "expected 'part pv.' LVM PV declaration in rendered output"
        );

        // Each VG must be declared with --useexisting since partition_disks
        // already created it before the OS installer runs.
        assert!(
            result.contains("volgroup vg0 --useexisting"),
            "expected 'volgroup vg0 --useexisting' in rendered output"
        );

        // The bootloader directive must specify the drive so GRUB is installed
        // to the correct MBR.
        assert!(
            result.contains("bootloader --boot-drive="),
            "expected 'bootloader --boot-drive=' in rendered output"
        );
    }
}
