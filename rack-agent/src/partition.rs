use anyhow::{Result, anyhow};
use common::disk_layout::{
    DiskConfig, DiskLayout, PartitionConfig, VolumeGroup, ZfsPool, partition_path,
};
use log::info;

use common::cnc::CncClient;

/// Partition disks according to the device's role disk layout
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Fetches the resolved disk layout from rack-director
/// 3. Applies the layout using parted, lvm, zfs, and mkfs
/// 4. Reports success or failure to rack-director
///
/// `plan_id` is forwarded to success/failure reports so rack-director can
/// discard stale reports from a previously-cancelled plan.
pub async fn partition_disks(client: &CncClient, plan_id: Option<i64>) -> Result<()> {
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
            client.action_failed(&uuid, &error_msg, plan_id).await?;
            return Err(e);
        }
    };

    info!("Retrieved disk layout with {} disk(s)", layout.disks.len());

    // Apply layout
    if let Err(e) = apply_disk_layout(&layout).await {
        let error_msg = format!("Disk partitioning failed: {}", e);
        log::error!("{}", error_msg);
        client.action_failed(&uuid, &error_msg, plan_id).await?;
        return Err(e);
    }

    info!("Disk partitioning completed, verifying layout...");

    // Verify the layout was applied correctly before reporting success
    match verify_disk_layout(&layout).await {
        Ok(()) => {
            info!("Disk layout verification passed");
            client.action_success(&uuid, plan_id).await?;
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Disk layout verification failed: {}", e);
            log::error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg, plan_id).await?;
            Err(e)
        }
    }
}

/// Apply a disk layout to the system
///
/// Execution order:
/// 0. Clean up any existing LVM state for VGs we are about to recreate
/// 1. If `wipe_all_disks` is true, erase partition info from every whole disk
/// 2. Wipe and partition each disk in the layout
/// 3. Wait for udev to settle
/// 4. Set up LVM volume groups and logical volumes
/// 5. Set up ZFS pools and datasets
/// 6. Format simple partitions (not LVM/ZFS)
async fn apply_disk_layout(layout: &DiskLayout) -> Result<()> {
    // Step 0: Clean up any existing LVM state on the target disks before wiping.
    // This prevents stale VGs/PVs (from a failed prior run) from blocking pvcreate/
    // vgcreate when the daemon retries without a reboot. Best-effort: errors ignored.
    // Pass an empty slice when wiping all disks so remove_lvm_on_disks removes every VG.
    let lvm_disks = if layout.wipe_all_disks {
        &[][..]
    } else {
        &layout.disks[..]
    };
    remove_lvm_on_disks(lvm_disks).await;

    // Step 1: Optionally wipe partition info from ALL disks on the machine.
    if layout.wipe_all_disks {
        wipe_all_disks().await?;
    }

    // Step 2: Wipe and partition each disk
    for disk in &layout.disks {
        wipe_and_partition_disk(disk).await?;
    }

    // Step 3: Wait for udev to settle after partition changes
    run_command("udevadm", &["settle", "--timeout=10"]).await?;

    // Step 4: LVM setup
    if let Some(ref volume_groups) = layout.volume_groups {
        for vg in volume_groups {
            setup_volume_group(vg, &layout.disks).await?;
        }
    }

    // Step 5: ZFS setup
    if let Some(ref zfs_pools) = layout.zfs_pools {
        for pool in zfs_pools {
            setup_zfs_pool(pool).await?;
        }
    }

    // Step 6: Format simple partitions (not LVM, not ZFS)
    for disk in &layout.disks {
        format_simple_partitions(disk, layout).await?;
    }

    Ok(())
}

// ========== Disk Operations ==========

/// Erase partition info (`wipefs --all --force` + `sgdisk --zap-all`) from every whole
/// disk on the machine. Does NOT create partition tables — that stays in
/// `wipe_and_partition_disk` for disks that are in the layout. Targeted disks get
/// re-wiped there; running these commands twice is idempotent and harmless.
async fn wipe_all_disks() -> Result<()> {
    let paths = crate::scan::list_disk_paths().await?;
    info!(
        "wipe_all_disks: erasing partition info from {} disk(s)",
        paths.len()
    );
    for dev in &paths {
        run_command("wipefs", &["--all", "--force", dev]).await?;
        run_command("sgdisk", &["--zap-all", dev]).await?;
    }
    Ok(())
}

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

    Ok(())
}

// ========== LVM Operations ==========

/// Remove LVM volume groups whose PVs reside on the given disks.
///
/// Resolves each disk path to its canonical device name (so by-path/by-id
/// symlinks match what `pvs` reports), then queries `pvs` to discover which
/// VGs have PVs on those devices. An empty `disks` slice means wipe_all_disks
/// is active — every VG on the system is removed.
///
/// Best-effort: errors are silently ignored because on a fresh system there
/// are no PVs to remove.
async fn remove_lvm_on_disks(disks: &[DiskConfig]) {
    let canonical: Vec<String> = disks
        .iter()
        .map(|d| {
            std::fs::canonicalize(&d.device)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| d.device.clone())
        })
        .collect();

    // Query all PV→VG mappings. pvs failure means no LVM state — nothing to do.
    let output = match tokio::process::Command::new("pvs")
        .args(["--noheadings", "-o", "pv_name,vg_name"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut vgs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pv), Some(vg)) = (parts.next(), parts.next()) else {
            continue;
        };
        if vg.is_empty() {
            continue;
        }
        // Empty canonical list = wipe_all_disks: match every VG.
        let on_target =
            canonical.is_empty() || canonical.iter().any(|d| pv.starts_with(d.as_str()));
        if on_target {
            vgs.insert(vg.to_string());
        }
    }

    for vg in &vgs {
        info!("Removing LVM volume group: {}", vg);
        let _ = run_command("vgchange", &["-an", vg]).await;
        let _ = run_command("vgremove", &["--force", vg]).await;
    }
}

/// Returns true if the logical volume size is a free-space percentage value.
///
/// `lvcreate` requires these sizes to be passed with the `-l` extents flag rather
/// than the `-L` absolute-size flag.  The agent treats `"rest"` as an alias for
/// `"100%FREE"` so that disk-layout configs can use either term consistently.
fn is_free_size(size: &str) -> bool {
    size == "100%FREE" || size == "rest"
}

/// Validate that at most one logical volume in a VG uses a free-space size
/// (`100%FREE` or `rest`).
///
/// Having two such LVs is nonsensical — only one can consume the remaining
/// free extents.  The agent automatically places the free-size LV last when
/// creating volumes (see `setup_volume_group`), so users do not need to
/// order their config manually.
fn validate_volume_group(vg: &VolumeGroup) -> Result<()> {
    let free_count = vg
        .logical_volumes
        .iter()
        .filter(|lv| is_free_size(&lv.size))
        .count();
    if free_count > 1 {
        return Err(anyhow!(
            "Volume group '{}' has {} logical volumes with size '100%FREE'/'rest', \
             but at most one is allowed",
            vg.name,
            free_count,
        ));
    }
    Ok(())
}

async fn setup_volume_group(vg: &VolumeGroup, disks: &[DiskConfig]) -> Result<()> {
    info!("Setting up LVM volume group: {}", vg.name);

    validate_volume_group(vg)?;

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

    // Create logical volumes, always processing the free-size LV last so that
    // `100%FREE` / `rest` is submitted to lvcreate after all fixed-size LVs
    // have claimed their space — regardless of the order in the config.
    let (fixed_lvs, free_lvs): (Vec<_>, Vec<_>) = vg
        .logical_volumes
        .iter()
        .partition(|lv| !is_free_size(&lv.size));
    let ordered_lvs = fixed_lvs.into_iter().chain(free_lvs);

    for lv in ordered_lvs {
        info!("Creating logical volume: {}/{}", vg.name, lv.name);

        // Use the extents flag (-l) for percentage-style sizes; lvcreate rejects them
        // with the absolute-size flag (-L).  validate_volume_group guarantees that
        // these values only appear on the last LV.
        if lv.size == "100%FREE" || lv.size == "rest" {
            run_command(
                "lvcreate",
                &[
                    "-l",
                    "100%FREE",
                    "--zero",
                    "y",
                    "--wipesignatures",
                    "y",
                    "-y",
                    "-n",
                    &lv.name,
                    &vg.name,
                ],
            )
            .await?;
        } else {
            run_command(
                "lvcreate",
                &[
                    "-L",
                    &lv.size,
                    "--zero",
                    "y",
                    "--wipesignatures",
                    "y",
                    "-y",
                    "-n",
                    &lv.name,
                    &vg.name,
                ],
            )
            .await?;
        }

        // Format the logical volume, unless it has no filesystem (e.g. a raw
        // LV consumed by Ceph or another subsystem).
        if let Some(ref fs) = lv.filesystem {
            let lv_path = format!("/dev/{}/{}", vg.name, lv.name);
            format_filesystem(&lv_path, fs).await?;
        }
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

async fn format_simple_partitions(disk: &DiskConfig, layout: &DiskLayout) -> Result<()> {
    // Build the set of partition labels on this disk that belong to a ZFS pool.
    // ZFS pool device refs use the format "DISK_LABEL-PARTITION_LABEL" (e.g. "DATA1-zfs1").
    let zfs_partition_labels: std::collections::HashSet<&str> =
        build_zfs_partition_labels(disk, layout);

    for (i, partition) in disk.partitions.iter().enumerate() {
        // Skip partitions that belong to LVM
        if partition.volume_group.is_some() {
            continue;
        }

        // Skip partitions that belong to ZFS
        if zfs_partition_labels.contains(partition.label.as_str()) {
            continue;
        }

        if let Some(ref fs) = partition.filesystem {
            let part_path = partition_path(&disk.device, i + 1);
            format_filesystem(&part_path, fs).await?;
        }
    }
    Ok(())
}

/// Build the set of partition labels on this disk that belong to a ZFS pool.
///
/// ZFS device refs use the format "DISK_LABEL-PARTITION_LABEL" (e.g. "DATA1-zfs1").
/// We match refs whose prefix equals `disk.device` and collect the partition label suffix.
fn build_zfs_partition_labels<'a>(
    disk: &'a DiskConfig,
    layout: &'a DiskLayout,
) -> std::collections::HashSet<&'a str> {
    let mut labels = std::collections::HashSet::new();
    let Some(ref zfs_pools) = layout.zfs_pools else {
        return labels;
    };
    let disk_label = disk.device.as_str();
    let prefix = format!("{}-", disk_label);
    for pool in zfs_pools {
        for device_ref in &pool.devices {
            if let Some(partition_label) = device_ref.strip_prefix(&prefix) {
                labels.insert(partition_label);
            }
        }
    }
    labels
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
    info!("Running: {} {}", cmd, args.join(" "));

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

/// Parse a partition size string into an absolute byte count, or `None` for "rest".
///
/// Returns `Some(bytes)` for fixed sizes and percentages, `None` for "rest".
/// Percentages are resolved against `usable_size`.
fn parse_partition_size(size_str: &str, usable_size: u64) -> Result<Option<u64>> {
    if size_str == "rest" || size_str == "*" {
        Ok(None)
    } else if size_str.ends_with('%') {
        let pct: f64 = size_str
            .trim_end_matches('%')
            .parse()
            .map_err(|_| anyhow!("Invalid percentage: '{}'", size_str))?;
        Ok(Some((usable_size as f64 * pct / 100.0) as u64))
    } else {
        Ok(Some(parse_size(size_str)?))
    }
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
        match parse_partition_size(&p.size, usable_size)? {
            None => rest_count += 1,
            Some(bytes) => fixed_total += bytes,
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
        let size = match parse_partition_size(&p.size, usable_size)? {
            None => rest_size,
            Some(bytes) => bytes,
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
    if let Some(ref volume_groups) = layout.volume_groups
        && !volume_groups.is_empty()
    {
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
            let found = stdout
                .lines()
                .any(|line| line.split_whitespace().next() == Some(vg.name.as_str()));
            if !found {
                return Err(anyhow!(
                    "Volume group '{}' not found after creation",
                    vg.name
                ));
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

    // ========== parse_partition_size tests ==========

    #[test]
    fn test_parse_partition_size_rest() {
        let result = parse_partition_size("rest", 1024 * 1024 * 1024).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_partition_size_star() {
        let result = parse_partition_size("*", 1024 * 1024 * 1024).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_partition_size_percentage() {
        let usable = 100 * 1024 * 1024 * 1024u64; // 100 GiB
        let result = parse_partition_size("50%", usable).unwrap();
        assert_eq!(result, Some(50 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_partition_size_invalid_percentage() {
        assert!(parse_partition_size("abc%", 1024).is_err());
    }

    #[test]
    fn test_parse_partition_size_fixed() {
        let result = parse_partition_size("512MiB", 1024 * 1024 * 1024).unwrap();
        assert_eq!(result, Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_parse_partition_size_invalid_fixed() {
        assert!(parse_partition_size("notasize", 1024).is_err());
    }

    // ========== build_zfs_partition_labels tests ==========

    #[test]
    fn test_build_zfs_partition_labels_matches_disk() {
        let disk = DiskConfig {
            device: "DATA1".to_string(),
            partition_table: "gpt".to_string(),
            partitions: vec![],
        };
        let layout = DiskLayout {
            disks: vec![disk.clone()],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "single".to_string(),
                devices: vec!["DATA1-zfs1".to_string(), "DATA2-zfs2".to_string()],
                datasets: vec![],
                properties: None,
            }]),
            wipe_all_disks: false,
        };
        let labels = build_zfs_partition_labels(&disk, &layout);
        assert!(labels.contains("zfs1"));
        // DATA2-zfs2 should NOT appear for DATA1's disk
        assert!(!labels.contains("zfs2"));
    }

    #[test]
    fn test_build_zfs_partition_labels_no_zfs_pools() {
        let disk = DiskConfig {
            device: "ROOT".to_string(),
            partition_table: "gpt".to_string(),
            partitions: vec![],
        };
        let layout = DiskLayout {
            disks: vec![disk.clone()],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let labels = build_zfs_partition_labels(&disk, &layout);
        assert!(labels.is_empty());
    }

    #[test]
    fn test_build_zfs_partition_labels_no_false_prefix_match() {
        // "DATA1-zfs1" should not match disk device "DATA" (shorter prefix without dash)
        let disk = DiskConfig {
            device: "DATA".to_string(),
            partition_table: "gpt".to_string(),
            partitions: vec![],
        };
        let layout = DiskLayout {
            disks: vec![disk.clone()],
            volume_groups: None,
            zfs_pools: Some(vec![ZfsPool {
                name: "tank".to_string(),
                vdev_type: "single".to_string(),
                devices: vec!["DATA1-zfs1".to_string()],
                datasets: vec![],
                properties: None,
            }]),
            wipe_all_disks: false,
        };
        let labels = build_zfs_partition_labels(&disk, &layout);
        assert!(labels.is_empty());
    }

    /// Deserializing a disk layout JSON with `"wipe_all_disks": true` must produce a
    /// `DiskLayout` where the field is `true`. This validates the serde path used when
    /// `partition_disks` receives the layout from rack-director over the network.
    #[test]
    fn test_wipe_all_disks_field_deserializes_true() {
        let json = r#"{
            "disks": [{
                "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
                "partition_table": "gpt",
                "partitions": []
            }],
            "wipe_all_disks": true
        }"#;

        let layout: DiskLayout = serde_json::from_str(json).unwrap();
        assert!(
            layout.wipe_all_disks,
            "wipe_all_disks must be true when set in JSON"
        );
    }

    // ========== LVM discovery cleanup tests ==========

    #[tokio::test]
    async fn test_remove_lvm_on_disks_empty() {
        // Should not panic on empty disk list
        remove_lvm_on_disks(&[]).await;
    }

    #[tokio::test]
    async fn test_remove_lvm_on_disks_all_when_empty() {
        // Empty slice = wipe_all_disks path. pvs will fail (no LVM in test env) — should not panic.
        remove_lvm_on_disks(&[]).await;
    }

    // ========== validate_volume_group tests ==========

    #[test]
    fn test_validate_volume_group_rest_on_last_lv_is_valid() {
        use common::disk_layout::LogicalVolume;
        let vg = VolumeGroup {
            name: "vg0".to_string(),
            logical_volumes: vec![
                LogicalVolume {
                    name: "data".to_string(),
                    size: "20G".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
                LogicalVolume {
                    name: "scratch".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
            ],
        };
        assert!(validate_volume_group(&vg).is_ok());
    }

    #[test]
    fn test_validate_volume_group_100pct_free_on_last_lv_is_valid() {
        use common::disk_layout::LogicalVolume;
        let vg = VolumeGroup {
            name: "vg0".to_string(),
            logical_volumes: vec![
                LogicalVolume {
                    name: "swap".to_string(),
                    size: "4G".to_string(),
                    filesystem: Some("swap".to_string()),
                    mount_point: None,
                },
                LogicalVolume {
                    name: "root".to_string(),
                    size: "100%FREE".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: None,
                },
            ],
        };
        assert!(validate_volume_group(&vg).is_ok());
    }

    /// A VG with a single `rest`-sized LV not at position zero is valid now that
    /// the agent auto-reorders — validate_volume_group only cares about count.
    #[test]
    fn test_validate_volume_group_rest_not_at_end_is_valid() {
        use common::disk_layout::LogicalVolume;
        let vg = VolumeGroup {
            name: "vg0".to_string(),
            logical_volumes: vec![
                LogicalVolume {
                    name: "scratch".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
                LogicalVolume {
                    name: "data".to_string(),
                    size: "10G".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
            ],
        };
        // One free-size LV is allowed regardless of position — setup_volume_group reorders.
        assert!(validate_volume_group(&vg).is_ok());
    }

    /// Two free-size LVs in the same VG must be rejected — only one can consume
    /// remaining free extents.
    #[test]
    fn test_validate_volume_group_two_free_size_lvs_is_error() {
        use common::disk_layout::LogicalVolume;
        let vg = VolumeGroup {
            name: "vg0".to_string(),
            logical_volumes: vec![
                LogicalVolume {
                    name: "first".to_string(),
                    size: "100%FREE".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
                LogicalVolume {
                    name: "second".to_string(),
                    size: "rest".to_string(),
                    filesystem: None,
                    mount_point: None,
                },
            ],
        };
        let err = validate_volume_group(&vg).unwrap_err();
        assert!(
            err.to_string().contains("2"),
            "error should report the count of offending LVs: {err}"
        );
        assert!(
            err.to_string().contains("vg0"),
            "error should name the VG: {err}"
        );
    }

    #[test]
    fn test_validate_volume_group_single_lv_with_rest_is_valid() {
        use common::disk_layout::LogicalVolume;
        let vg = VolumeGroup {
            name: "vg0".to_string(),
            logical_volumes: vec![LogicalVolume {
                name: "root".to_string(),
                size: "100%FREE".to_string(),
                filesystem: Some("ext4".to_string()),
                mount_point: None,
            }],
        };
        assert!(validate_volume_group(&vg).is_ok());
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
