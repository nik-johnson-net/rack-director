use anyhow::{Result, anyhow};
use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig, VolumeGroup, ZfsPool};
use log::{debug, info};

use crate::client::RackDirector;

/// Partition disks according to the device's role disk layout
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Fetches the resolved disk layout from rack-director
/// 3. Applies the layout using parted, lvm, zfs, and mkfs
/// 4. Reports success or failure to rack-director
pub async fn partition_disks(client: &RackDirector) -> Result<()> {
    info!("Starting disk partitioning...");

    // Get device UUID
    let hardware_info = crate::scan::read_dmi_for_uuid().await?;
    let uuid =
        hardware_info.ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Device UUID: {}", uuid);

    // Fetch disk layout from rack-director
    info!("Fetching disk layout from rack-director...");
    let layout = match client.get_disk_layout(&uuid).await {
        Ok(layout) => layout,
        Err(e) => {
            let error_msg = format!("Failed to fetch disk layout: {}", e);
            log::error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg).await?;
            return Err(e);
        }
    };

    info!("Retrieved disk layout with {} disk(s)", layout.disks.len());

    // Apply layout
    if let Err(e) = apply_disk_layout(&layout).await {
        let error_msg = format!("Disk partitioning failed: {}", e);
        log::error!("{}", error_msg);
        client.action_failed(&uuid, &error_msg).await?;
        return Err(e);
    }

    info!("Disk partitioning completed, verifying layout...");

    // Verify the layout was applied correctly before reporting success
    match verify_disk_layout(&layout).await {
        Ok(()) => {
            info!("Disk layout verification passed");
            client.action_success(&uuid).await?;
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Disk layout verification failed: {}", e);
            log::error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg).await?;
            Err(e)
        }
    }
}

/// Apply a disk layout to the system
///
/// Execution order:
/// 1. Wipe and partition each disk
/// 2. Wait for udev to settle
/// 3. Set up LVM volume groups and logical volumes
/// 4. Set up ZFS pools and datasets
/// 5. Format simple partitions (not LVM/ZFS)
async fn apply_disk_layout(layout: &DiskLayout) -> Result<()> {
    // Step 1: Wipe and partition each disk
    for disk in &layout.disks {
        wipe_and_partition_disk(disk).await?;
    }

    // Step 2: Wait for udev to settle
    run_command("udevadm", &["settle", "--timeout=10"]).await?;

    // Step 3: LVM setup
    if let Some(ref volume_groups) = layout.volume_groups {
        for vg in volume_groups {
            setup_volume_group(vg, &layout.disks).await?;
        }
    }

    // Step 4: ZFS setup
    if let Some(ref zfs_pools) = layout.zfs_pools {
        for pool in zfs_pools {
            setup_zfs_pool(pool).await?;
        }
    }

    // Step 5: Format simple partitions (not LVM, not ZFS)
    for disk in &layout.disks {
        format_simple_partitions(disk).await?;
    }

    Ok(())
}

// ========== Disk Operations ==========

async fn wipe_and_partition_disk(disk: &DiskConfig) -> Result<()> {
    let device = &disk.device;
    info!("Preparing disk: {}", device);

    // Wipe existing signatures
    run_command("wipefs", &["--all", "--force", device]).await?;
    run_command("sgdisk", &["--zap-all", device]).await?;

    // Create partition table
    run_command("parted", &["-s", device, "mklabel", &disk.partition_table]).await?;

    // Get disk size for percentage/rest calculations
    let disk_size = get_disk_size(device).await?;
    let offsets = calculate_partition_offsets(&disk.partitions, disk_size)?;

    // Create partitions
    for (i, (partition, (start, end))) in disk.partitions.iter().zip(offsets.iter()).enumerate() {
        let part_num = i + 1;
        let fs_hint = partition
            .filesystem
            .as_deref()
            .map(fs_type_hint)
            .unwrap_or("ext4");

        let start_str = format!("{}B", start);
        let end_str = format!("{}B", end);

        run_command(
            "parted",
            &[
                "-s",
                device,
                "mkpart",
                &partition.label,
                fs_hint,
                &start_str,
                &end_str,
            ],
        )
        .await?;

        // Set flags
        if let Some(ref flags) = partition.flags {
            let num_str = part_num.to_string();
            for flag in flags {
                run_command("parted", &["-s", device, "set", &num_str, flag, "on"]).await?;
            }
        }
    }

    // Notify kernel of partition changes
    run_command("partprobe", &[device]).await?;

    Ok(())
}

// ========== LVM Operations ==========

async fn setup_volume_group(vg: &VolumeGroup, disks: &[DiskConfig]) -> Result<()> {
    info!("Setting up LVM volume group: {}", vg.name);

    // Find all partitions that belong to this VG
    let mut pv_devices = Vec::new();
    for disk in disks {
        for (i, partition) in disk.partitions.iter().enumerate() {
            if partition.volume_group.as_deref() == Some(&vg.name) {
                pv_devices.push(partition_path(&disk.device, i + 1));
            }
        }
    }

    if pv_devices.is_empty() {
        return Err(anyhow!(
            "No partitions found for volume group '{}'",
            vg.name
        ));
    }

    // Create physical volumes
    for pv in &pv_devices {
        run_command("pvcreate", &["-ff", "-y", pv]).await?;
    }

    // Create volume group
    let mut vgcreate_args = vec![vg.name.as_str()];
    let pv_refs: Vec<&str> = pv_devices.iter().map(|s| s.as_str()).collect();
    vgcreate_args.extend(pv_refs);
    run_command("vgcreate", &vgcreate_args).await?;

    // Create logical volumes
    let lv_count = vg.logical_volumes.len();
    for (i, lv) in vg.logical_volumes.iter().enumerate() {
        let is_last = i == lv_count - 1;
        info!("Creating logical volume: {}/{}", vg.name, lv.name);

        if is_last && (lv.size == "100%FREE" || lv.size == "rest") {
            run_command("lvcreate", &["-l", "100%FREE", "-n", &lv.name, &vg.name]).await?;
        } else {
            run_command("lvcreate", &["-L", &lv.size, "-n", &lv.name, &vg.name]).await?;
        }

        // Format the logical volume
        let lv_path = format!("/dev/{}/{}", vg.name, lv.name);
        format_filesystem(&lv_path, &lv.filesystem).await?;
    }

    Ok(())
}

// ========== ZFS Operations ==========

async fn setup_zfs_pool(pool: &ZfsPool) -> Result<()> {
    info!("Setting up ZFS pool: {}", pool.name);

    let mut args = vec![
        "create".to_string(),
        "-f".to_string(),
        "-o".to_string(),
        "ashift=12".to_string(),
    ];

    // Add pool-level properties
    if let Some(ref properties) = pool.properties {
        for (key, value) in properties {
            args.push("-o".to_string());
            args.push(format!("{}={}", key, value));
        }
    }

    args.push(pool.name.clone());

    // Add vdev type (skip for "single")
    if pool.vdev_type != "single" {
        args.push(pool.vdev_type.clone());
    }

    // Add devices
    args.extend(pool.devices.iter().cloned());

    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_command("zpool", &args_refs).await?;

    // Create datasets
    for dataset in &pool.datasets {
        let full_name = format!("{}/{}", pool.name, dataset.name);
        info!("Creating ZFS dataset: {}", full_name);

        let mut ds_args = vec!["create".to_string()];

        // Zvol creation
        if let Some(ref zvol_size) = dataset.zvol_size {
            ds_args.push("-V".to_string());
            ds_args.push(zvol_size.clone());
        }

        if let Some(ref mount_point) = dataset.mount_point {
            ds_args.push("-o".to_string());
            ds_args.push(format!("mountpoint={}", mount_point));
        }

        if let Some(ref properties) = dataset.properties {
            for (key, value) in properties {
                ds_args.push("-o".to_string());
                ds_args.push(format!("{}={}", key, value));
            }
        }

        ds_args.push(full_name);

        let ds_refs: Vec<&str> = ds_args.iter().map(|s| s.as_str()).collect();
        run_command("zfs", &ds_refs).await?;
    }

    Ok(())
}

// ========== Simple Partition Formatting ==========

async fn format_simple_partitions(disk: &DiskConfig) -> Result<()> {
    for (i, partition) in disk.partitions.iter().enumerate() {
        // Skip partitions that belong to LVM
        if partition.volume_group.is_some() {
            continue;
        }

        if let Some(ref fs) = partition.filesystem {
            let part_path = partition_path(&disk.device, i + 1);
            format_filesystem(&part_path, fs).await?;
        }
    }
    Ok(())
}

async fn format_filesystem(device: &str, filesystem: &str) -> Result<()> {
    info!("Formatting {} as {}", device, filesystem);

    match filesystem {
        "ext4" => run_command("mkfs.ext4", &["-F", "-q", device]).await,
        "ext3" => run_command("mkfs.ext3", &["-F", "-q", device]).await,
        "xfs" => run_command("mkfs.xfs", &["-f", "-q", device]).await,
        "btrfs" => run_command("mkfs.btrfs", &["-f", device]).await,
        "vfat" | "fat32" => run_command("mkfs.vfat", &["-F", "32", device]).await,
        "swap" => run_command("mkswap", &[device]).await,
        _ => Err(anyhow!("Unsupported filesystem: {}", filesystem)),
    }
}

// ========== Helper Functions ==========

/// Execute a command and return error on non-zero exit
async fn run_command(cmd: &str, args: &[&str]) -> Result<()> {
    debug!("Running: {} {}", cmd, args.join(" "));

    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute {}: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!(
            "{} failed (exit {}): stderr={}, stdout={}",
            cmd,
            output.status.code().unwrap_or(-1),
            stderr.trim(),
            stdout.trim()
        ));
    }

    Ok(())
}

/// Generate partition device path
///
/// For SATA/SCSI: /dev/sda + 1 = /dev/sda1
/// For NVMe: /dev/nvme0n1 + 1 = /dev/nvme0n1p1
/// For device-mapper: /dev/dm-0 + 1 = /dev/dm-0p1
fn partition_path(disk: &str, partition_num: usize) -> String {
    // NVMe and device-mapper disks need a 'p' separator
    let needs_p = disk.chars().last().is_some_and(|c| c.is_ascii_digit());
    if needs_p {
        format!("{}p{}", disk, partition_num)
    } else {
        format!("{}{}", disk, partition_num)
    }
}

/// Parse a size string to bytes
///
/// Supports: "512MiB", "100GiB", "1TiB", "500GB", "100G", "50%", "rest"
/// Returns error for "rest" and percentage strings (handled separately by calculate_partition_offsets)
fn parse_size(size_str: &str) -> Result<u64> {
    let size_str = size_str.trim();

    if size_str == "rest" || size_str.ends_with('%') {
        return Err(anyhow!("Cannot parse '{}' as absolute size", size_str));
    }

    let (num_str, multiplier) = if let Some(n) = size_str.strip_suffix("TiB") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = size_str.strip_suffix("GiB") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = size_str.strip_suffix("MiB") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = size_str.strip_suffix("KiB") {
        (n, 1024u64)
    } else if let Some(n) = size_str.strip_suffix("TB") {
        (n, 1000u64 * 1000 * 1000 * 1000)
    } else if let Some(n) = size_str.strip_suffix("GB") {
        (n, 1000u64 * 1000 * 1000)
    } else if let Some(n) = size_str.strip_suffix("MB") {
        (n, 1000u64 * 1000)
    } else if let Some(n) = size_str.strip_suffix("KB") {
        (n, 1000u64)
    } else if let Some(n) = size_str.strip_suffix("T") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = size_str.strip_suffix("G") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = size_str.strip_suffix("M") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = size_str.strip_suffix("K") {
        (n, 1024u64)
    } else if let Some(n) = size_str.strip_suffix("B") {
        (n, 1u64)
    } else {
        // Assume bytes
        (size_str, 1u64)
    };

    let num: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid size number: '{}'", num_str))?;

    Ok((num * multiplier as f64) as u64)
}

/// Calculate partition start/end byte offsets
///
/// Handles fixed sizes, percentages, and "rest" (remaining space).
/// Aligns partitions to 1MiB boundaries for optimal performance.
fn calculate_partition_offsets(
    partitions: &[PartitionConfig],
    disk_size_bytes: u64,
) -> Result<Vec<(u64, u64)>> {
    const ALIGN: u64 = 1024 * 1024; // 1 MiB alignment

    // Start after first 1MiB (for GPT header + alignment)
    let mut current = ALIGN;
    // Reserve last 1MiB for backup GPT
    let usable_end = disk_size_bytes.saturating_sub(ALIGN);

    if usable_end <= current {
        return Err(anyhow!("Disk too small: {} bytes", disk_size_bytes));
    }

    let usable_size = usable_end - current;

    // First pass: calculate fixed sizes and percentages, find "rest" partitions
    let mut rest_count = 0u32;
    let mut fixed_total = 0u64;

    for p in partitions {
        if p.size == "rest" {
            rest_count += 1;
        } else if p.size.ends_with('%') {
            let pct: f64 = p
                .size
                .trim_end_matches('%')
                .parse()
                .map_err(|_| anyhow!("Invalid percentage: '{}'", p.size))?;
            fixed_total += (usable_size as f64 * pct / 100.0) as u64;
        } else {
            fixed_total += parse_size(&p.size)?;
        }
    }

    if rest_count > 1 {
        return Err(anyhow!("Multiple 'rest' partitions are not allowed"));
    }

    if fixed_total > usable_size {
        return Err(anyhow!(
            "Partition sizes ({} bytes) exceed usable disk space ({} bytes)",
            fixed_total,
            usable_size
        ));
    }

    let rest_size = if rest_count > 0 {
        usable_size - fixed_total
    } else {
        0
    };

    // Second pass: assign offsets
    let mut offsets = Vec::new();

    for p in partitions {
        let size = if p.size == "rest" {
            rest_size
        } else if p.size.ends_with('%') {
            let pct: f64 = p.size.trim_end_matches('%').parse().unwrap();
            (usable_size as f64 * pct / 100.0) as u64
        } else {
            parse_size(&p.size)?
        };

        // Align size up to boundary
        let aligned_size = size.div_ceil(ALIGN) * ALIGN;
        let end = std::cmp::min(current + aligned_size, usable_end);

        offsets.push((current, end - 1)); // parted uses inclusive end
        current = end;
    }

    Ok(offsets)
}

/// Get disk size in bytes via lsblk
async fn get_disk_size(device: &str) -> Result<u64> {
    let output = tokio::process::Command::new("lsblk")
        .args(["-b", "-d", "-n", "-o", "SIZE", device])
        .output()
        .await
        .map_err(|e| anyhow!("Failed to run lsblk: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!("lsblk failed for {}", device));
    }

    let size_str = String::from_utf8_lossy(&output.stdout);
    let size: u64 = size_str
        .trim()
        .parse()
        .map_err(|_| anyhow!("Failed to parse disk size: '{}'", size_str.trim()))?;

    Ok(size)
}

/// Verify that the disk layout was applied correctly.
///
/// Checks:
/// - Partition tables exist on each disk (via sfdisk --json)
/// - LVM volume groups are present if configured (via vgs --noheadings)
async fn verify_disk_layout(layout: &DiskLayout) -> Result<()> {
    // Check partition tables on each disk
    for disk in &layout.disks {
        let output = tokio::process::Command::new("sfdisk")
            .args(["--json", &disk.device])
            .output()
            .await
            .map_err(|e| anyhow!("Failed to run sfdisk: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "sfdisk verification failed for {}: {}",
                disk.device,
                stderr.trim()
            ));
        }

        // Parse JSON to verify valid partition table data was written
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str::<serde_json::Value>(&stdout)
            .map_err(|e| anyhow!("sfdisk output is not valid JSON for {}: {}", disk.device, e))?;
    }

    // Check LVM volume groups if any
    if let Some(ref volume_groups) = layout.volume_groups {
        if !volume_groups.is_empty() {
            let output = tokio::process::Command::new("vgs")
                .args(["--noheadings"])
                .output()
                .await
                .map_err(|e| anyhow!("Failed to run vgs: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("vgs verification failed: {}", stderr.trim()));
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            for vg in volume_groups {
                if !stdout.contains(&vg.name) {
                    return Err(anyhow!(
                        "Volume group '{}' not found after creation",
                        vg.name
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Map filesystem type to parted's fs-type hint
fn fs_type_hint(filesystem: &str) -> &str {
    match filesystem {
        "ext4" | "ext3" | "ext2" => "ext4",
        "xfs" => "xfs",
        "btrfs" => "btrfs",
        "vfat" | "fat32" | "fat16" => "fat32",
        "swap" => "linux-swap",
        "ntfs" => "ntfs",
        _ => "ext4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== partition_path tests ==========

    #[test]
    fn test_partition_path_sata() {
        assert_eq!(partition_path("/dev/sda", 1), "/dev/sda1");
        assert_eq!(partition_path("/dev/sda", 2), "/dev/sda2");
        assert_eq!(partition_path("/dev/sdb", 10), "/dev/sdb10");
    }

    #[test]
    fn test_partition_path_nvme() {
        assert_eq!(partition_path("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
        assert_eq!(partition_path("/dev/nvme0n1", 2), "/dev/nvme0n1p2");
        assert_eq!(partition_path("/dev/nvme1n1", 3), "/dev/nvme1n1p3");
    }

    #[test]
    fn test_partition_path_dm() {
        assert_eq!(partition_path("/dev/dm-0", 1), "/dev/dm-0p1");
    }

    // ========== parse_size tests ==========

    #[test]
    fn test_parse_size_mib() {
        assert_eq!(parse_size("512MiB").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_size("1MiB").unwrap(), 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gib() {
        assert_eq!(parse_size("100GiB").unwrap(), 100 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1GiB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_shorthand() {
        assert_eq!(parse_size("50G").unwrap(), 50 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("100M").unwrap(), 100 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gb() {
        assert_eq!(parse_size("500GB").unwrap(), 500 * 1000 * 1000 * 1000);
    }

    #[test]
    fn test_parse_size_rest_error() {
        assert!(parse_size("rest").is_err());
    }

    #[test]
    fn test_parse_size_percentage_error() {
        assert!(parse_size("50%").is_err());
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc").is_err());
    }

    // ========== calculate_partition_offsets tests ==========

    #[test]
    fn test_calculate_offsets_fixed_and_rest() {
        let partitions = vec![
            PartitionConfig {
                label: "efi".to_string(),
                size: "512MiB".to_string(),
                filesystem: Some("vfat".to_string()),
                mount_point: Some("/boot/efi".to_string()),
                flags: Some(vec!["boot".to_string(), "esp".to_string()]),
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
        ];

        // 100 GiB disk
        let disk_size = 100 * 1024 * 1024 * 1024u64;
        let offsets = calculate_partition_offsets(&partitions, disk_size).unwrap();

        assert_eq!(offsets.len(), 2);
        // First partition starts at 1MiB
        assert_eq!(offsets[0].0, 1024 * 1024);
        // Second partition should take the rest
        assert!(offsets[1].1 > offsets[1].0);
    }

    #[test]
    fn test_calculate_offsets_percentage() {
        let partitions = vec![
            PartitionConfig {
                label: "part1".to_string(),
                size: "50%".to_string(),
                filesystem: Some("ext4".to_string()),
                mount_point: None,
                flags: None,
                volume_group: None,
            },
            PartitionConfig {
                label: "part2".to_string(),
                size: "50%".to_string(),
                filesystem: Some("ext4".to_string()),
                mount_point: None,
                flags: None,
                volume_group: None,
            },
        ];

        let disk_size = 100 * 1024 * 1024 * 1024u64;
        let offsets = calculate_partition_offsets(&partitions, disk_size).unwrap();

        assert_eq!(offsets.len(), 2);
        // Both should be roughly equal
        let size1 = offsets[0].1 - offsets[0].0;
        let size2 = offsets[1].1 - offsets[1].0;
        assert!((size1 as i64 - size2 as i64).unsigned_abs() < 2 * 1024 * 1024);
    }

    #[test]
    fn test_calculate_offsets_overflow_error() {
        let partitions = vec![PartitionConfig {
            label: "too-big".to_string(),
            size: "200GiB".to_string(),
            filesystem: Some("ext4".to_string()),
            mount_point: None,
            flags: None,
            volume_group: None,
        }];

        let disk_size = 100 * 1024 * 1024 * 1024u64;
        assert!(calculate_partition_offsets(&partitions, disk_size).is_err());
    }

    #[test]
    fn test_calculate_offsets_multiple_rest_error() {
        let partitions = vec![
            PartitionConfig {
                label: "rest1".to_string(),
                size: "rest".to_string(),
                filesystem: Some("ext4".to_string()),
                mount_point: None,
                flags: None,
                volume_group: None,
            },
            PartitionConfig {
                label: "rest2".to_string(),
                size: "rest".to_string(),
                filesystem: Some("ext4".to_string()),
                mount_point: None,
                flags: None,
                volume_group: None,
            },
        ];

        let disk_size = 100 * 1024 * 1024 * 1024u64;
        let result = calculate_partition_offsets(&partitions, disk_size);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Multiple 'rest'"));
    }

    #[test]
    fn test_calculate_offsets_disk_too_small() {
        let partitions = vec![PartitionConfig {
            label: "small".to_string(),
            size: "rest".to_string(),
            filesystem: Some("ext4".to_string()),
            mount_point: None,
            flags: None,
            volume_group: None,
        }];

        assert!(calculate_partition_offsets(&partitions, 1).is_err());
    }

    // ========== fs_type_hint tests ==========

    #[test]
    fn test_fs_type_hint() {
        assert_eq!(fs_type_hint("ext4"), "ext4");
        assert_eq!(fs_type_hint("xfs"), "xfs");
        assert_eq!(fs_type_hint("vfat"), "fat32");
        assert_eq!(fs_type_hint("swap"), "linux-swap");
        assert_eq!(fs_type_hint("btrfs"), "btrfs");
        assert_eq!(fs_type_hint("fat32"), "fat32");
    }
}
