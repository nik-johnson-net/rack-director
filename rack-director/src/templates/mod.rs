use crate::operating_systems::OperatingSystem;
use crate::roles::Role;
use anyhow::Result;
use common::disk_layout::{DiskLayout, partition_path};
use handlebars::Handlebars;
use serde_json::json;
use uuid::Uuid;

/// Network information for a device
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub mac_address: String,
    pub ip_address: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub netmask: String,
    pub prefix_length: u8,
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
/// - `{{ this.filesystem }}` - Filesystem type
/// - `{{ this.mount_point }}` - Mount point, or null
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogicalVolumeContext {
    pub device: String,
    pub device_name: String,
    pub vg_name: String,
    pub lv_name: String,
    pub size: String,
    pub filesystem: String,
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

/// Render an install script template with device-specific variables
///
/// Available template variables:
/// - {{ device.uuid }} - Device UUID
/// - {{ device.hostname }} - Device hostname
/// - {{ device.mac_address }} - Primary MAC address
/// - {{ device.ip_address }} - IP address (static or DHCP lease)
/// - {{ device.gateway }} - Network gateway
/// - {{ device.dns_servers }} - DNS servers (space-separated)
/// - {{ device.netmask }} - Network netmask
/// - {{ device.boot_mode }} - Firmware mode ("bios", "uefi", or "" if not detected)
/// - {{ device.is_uefi }} - Boolean true if UEFI firmware
/// - {{ device.is_bios }} - Boolean true if BIOS firmware
/// - {{ role.name }} - Role name
/// - {{ role.disk_layout }} - Disk layout as JSON (raw, unresolved)
/// - {{ os.name }} - OS name
/// - {{ os.version }} - OS version
/// - {{ config.* }} - Any custom config from role.config_template
/// - {{ partitions }} - Array of resolved partition contexts (see PartitionContext)
/// - {{ logical_volumes }} - Array of resolved LVM logical volume contexts (see LogicalVolumeContext)
pub fn render_install_script(
    template: &str,
    device: &DeviceInfo,
    role: &Role,
    os: &OperatingSystem,
    network: &NetworkInfo,
    disk_layout: &DiskLayout,
) -> Result<String> {
    let mut handlebars = Handlebars::new();

    // Don't HTML-escape output (we're generating config files, not HTML)
    handlebars.register_escape_fn(handlebars::no_escape);

    let (partitions, logical_volumes) = build_disk_layout_context(disk_layout);

    // Build firmware mode template variables
    let boot_mode_str = device
        .boot_mode
        .map(|m| match m {
            common::FirmwareMode::Bios => "bios",
            common::FirmwareMode::Uefi => "uefi",
        })
        .unwrap_or("");
    let is_uefi = device.boot_mode == Some(common::FirmwareMode::Uefi);
    let is_bios = device.boot_mode == Some(common::FirmwareMode::Bios);

    // Build context with all available variables
    let context = json!({
        "device": {
            "uuid": device.uuid.to_string(),
            "hostname": device.hostname.as_deref().unwrap_or("unknown"),
            "mac_address": network.mac_address,
            "ip_address": network.ip_address,
            "gateway": network.gateway,
            "dns_servers": network.dns_servers.join(" "),
            "netmask": network.netmask,
            "prefix_length": network.prefix_length,
            "boot_mode": boot_mode_str,
            "is_uefi": is_uefi,
            "is_bios": is_bios,
        },
        "role": {
            "name": role.name,
            "disk_layout": role.disk_layout,
        },
        "os": {
            "name": os.name,
            "version": os.version,
        },
        "config": role.config_template,
        "partitions": partitions,
        "logical_volumes": logical_volumes,
    });

    Ok(handlebars.render_template(template, &context)?)
}

/// Render an install script template using OSM-resolved context.
///
/// Similar to `render_install_script` but takes OS name/version as strings
/// and template variables for the config context.
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

    let context = json!({
        "device": {
            "uuid": device.uuid.to_string(),
            "hostname": device.hostname.as_deref().unwrap_or("unknown"),
            "mac_address": network.mac_address,
            "ip_address": network.ip_address,
            "gateway": network.gateway,
            "dns_servers": network.dns_servers.join(","),
            "dns_servers_csv": network.dns_servers.join(","),
            "netmask": network.netmask,
            "prefix_length": network.prefix_length,
            "boot_mode": boot_mode_str,
            "is_uefi": is_uefi,
            "is_bios": is_bios,
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

    fn make_role(disk_layout: DiskLayout) -> Role {
        Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout,
            config_template: None,
            firmware_mode: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn make_os() -> OperatingSystem {
        OperatingSystem {
            id: Some(1),
            name: "Ubuntu".to_string(),
            version: "24.04".to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn make_network() -> NetworkInfo {
        NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            netmask: "255.255.255.0".to_string(),
            prefix_length: 24,
        }
    }

    fn empty_disk_layout() -> DiskLayout {
        DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    #[test]
    fn test_render_simple_template() {
        let template = "hostname: {{ device.hostname }}";
        let result = render_install_script(
            template,
            &make_device(),
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "hostname: server01");
    }

    #[test]
    fn test_render_network_template() {
        let template = r#"
network:
  address: {{ device.ip_address }}
  gateway: {{ device.gateway }}
  netmask: {{ device.netmask }}
  dns: {{ device.dns_servers }}
"#;
        let result = render_install_script(
            template,
            &make_device(),
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert!(result.contains("address: 10.0.0.100"));
        assert!(result.contains("gateway: 10.0.0.1"));
        assert!(result.contains("dns: 8.8.8.8 8.8.4.4"));
    }

    #[test]
    fn test_render_with_custom_config() {
        let template = "{{#each config.packages}}{{ this }} {{/each}}";
        let role = Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout: empty_disk_layout(),
            config_template: Some(json!({
                "packages": ["nginx", "postgresql", "redis"]
            })),
            firmware_mode: None,
            created_at: None,
            updated_at: None,
        };
        let result = render_install_script(
            template,
            &make_device(),
            &role,
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "nginx postgresql redis ");
    }

    #[test]
    fn test_render_debian_preseed() {
        let template = r#"
d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}
"#;
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("debian-server".to_string()),
            boot_mode: None,
        };
        let network = NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            netmask: "255.255.255.0".to_string(),
            prefix_length: 24,
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &network,
            &empty_disk_layout(),
        )
        .unwrap();
        assert!(result.contains("d-i netcfg/get_hostname string debian-server"));
        assert!(result.contains("d-i netcfg/get_ipaddress string 10.0.0.100"));
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
                        filesystem: "ext4".to_string(),
                        mount_point: Some("/".to_string()),
                    },
                    LogicalVolume {
                        name: "swap".to_string(),
                        size: "8G".to_string(),
                        filesystem: "swap".to_string(),
                        mount_point: None,
                    },
                ],
            }]),
            zfs_pools: None,
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
        assert_eq!(logical_volumes[0].filesystem, "ext4");
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
                    filesystem: "xfs".to_string(),
                    mount_point: Some("/home".to_string()),
                }],
            }]),
            zfs_pools: None,
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
    fn test_render_partitions_in_template() {
        let template = "{{#each partitions}}{{ this.device }} {{ this.mount_point }}\n{{/each}}";
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
                        flags: None,
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
        };

        let result = render_install_script(
            template,
            &make_device(),
            &make_role(layout.clone()),
            &make_os(),
            &make_network(),
            &layout,
        )
        .unwrap();

        assert!(result.contains("/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part1 /boot/efi"));
        assert!(result.contains("/dev/disk/by-path/pci-0000:00:1f.2-ata-1-part2 /"));
    }

    #[test]
    fn test_render_logical_volumes_in_template() {
        let template =
            "{{#each logical_volumes}}{{ this.device }} {{ this.mount_point }}\n{{/each}}";
        let layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "lvm".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                    flags: Some(vec!["lvm".to_string()]),
                    volume_group: Some("vg0".to_string()),
                }],
            }],
            volume_groups: Some(vec![VolumeGroup {
                name: "vg0".to_string(),
                logical_volumes: vec![LogicalVolume {
                    name: "root".to_string(),
                    size: "50G".to_string(),
                    filesystem: "ext4".to_string(),
                    mount_point: Some("/".to_string()),
                }],
            }]),
            zfs_pools: None,
        };

        let result = render_install_script(
            template,
            &make_device(),
            &make_role(layout.clone()),
            &make_os(),
            &make_network(),
            &layout,
        )
        .unwrap();

        assert!(result.contains("/dev/vg0/root /"));
    }

    #[test]
    fn test_render_boot_mode_uefi() {
        let template = "{{ device.boot_mode }}";
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: Some(common::FirmwareMode::Uefi),
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "uefi");
    }

    #[test]
    fn test_render_boot_mode_bios() {
        let template = "{{ device.boot_mode }}";
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: Some(common::FirmwareMode::Bios),
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "bios");
    }

    #[test]
    fn test_render_boot_mode_unknown() {
        let template = "{{ device.boot_mode }}";
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: None,
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_is_uefi_true() {
        let template = "{{#if device.is_uefi}}UEFI{{else}}NOT_UEFI{{/if}}";
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: Some(common::FirmwareMode::Uefi),
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "UEFI");
    }

    #[test]
    fn test_render_is_bios_true() {
        let template = "{{#if device.is_bios}}BIOS{{else}}NOT_BIOS{{/if}}";
        let device = DeviceInfo {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            hostname: Some("server01".to_string()),
            boot_mode: Some(common::FirmwareMode::Bios),
        };
        let result = render_install_script(
            template,
            &device,
            &make_role(empty_disk_layout()),
            &make_os(),
            &make_network(),
            &empty_disk_layout(),
        )
        .unwrap();
        assert_eq!(result, "BIOS");
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
            &empty_disk_layout(),
            "",
        )
        .unwrap();
        assert_eq!(result, "hostname: server01\nos: Ubuntu 22.04");
    }
}
