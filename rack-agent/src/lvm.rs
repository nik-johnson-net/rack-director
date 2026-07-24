//! Shared LVM/dm cleanup logic used before disk operations that require the
//! target disks to be free of active PVs/VGs (whole-disk erase, repartitioning).

use std::collections::HashSet;

use anyhow::{Result, anyhow};
use log::{info, warn};

/// Remove LVM volume groups whose PVs reside on the given disks.
///
/// Resolves each disk path to its canonical device name (so by-path/by-id
/// symlinks match what `pvs` reports), then queries `pvs` to discover which
/// VGs have PVs on those devices. An empty `disk_paths` slice means "clean up
/// everything" — every VG on the system is removed. This is used both for
/// whole-machine operations (`wipe_disks`, `partition_disks`'s
/// `wipe_all_disks`) and for cleanup scoped to a specific set of disks before
/// re-partitioning them.
///
/// Best-effort: every failure (the `pvs` query itself, or an individual
/// `vgchange`/`vgremove`) is logged as a warning and does not stop the
/// cleanup — a fresh system with no PVs is the common case, and callers treat
/// this step as advisory (the downstream disk operation will surface its own,
/// clearer error if stale LVM state actually blocks it).
pub async fn remove_lvm_on_disks(disk_paths: &[String]) {
    let canonical: Vec<String> = disk_paths.iter().map(|d| canonicalize_or_self(d)).collect();

    let output = match tokio::process::Command::new("pvs")
        .args(["--noheadings", "-o", "pv_name,vg_name"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            warn!(
                "pvs query failed (exit {}), assuming no LVM state to clean up: {}",
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return;
        }
        Err(e) => {
            warn!(
                "Failed to run pvs, assuming no LVM state to clean up: {}",
                e
            );
            return;
        }
    };

    let vgs = vgs_on_disks(&String::from_utf8_lossy(&output.stdout), &canonical);

    for vg in &vgs {
        info!("Removing LVM volume group: {}", vg);
        if let Err(e) = run_command("vgchange", &["-an", vg]).await {
            warn!("vgchange -an {} failed (continuing): {}", vg, e);
        }
        if let Err(e) = run_command("vgremove", &["--force", vg]).await {
            warn!("vgremove --force {} failed (continuing): {}", vg, e);
        }
    }
}

fn canonicalize_or_self(disk_path: &str) -> String {
    std::fs::canonicalize(disk_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| disk_path.to_string())
}

/// Parse `pvs --noheadings -o pv_name,vg_name` output and return the set of
/// VG names that have at least one PV on `canonical_disks`.
///
/// An empty `canonical_disks` matches every PV (the whole-machine case).
fn vgs_on_disks(pvs_output: &str, canonical_disks: &[String]) -> HashSet<String> {
    let mut vgs = HashSet::new();
    for line in pvs_output.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pv), Some(vg)) = (parts.next(), parts.next()) else {
            continue;
        };
        if vg.is_empty() {
            continue;
        }
        let on_target = canonical_disks.is_empty()
            || canonical_disks
                .iter()
                .any(|d| pv_is_on_disk(pv, d.as_str()));
        if on_target {
            vgs.insert(vg.to_string());
        }
    }
    vgs
}

/// Returns true if `pv` (a PV device path reported by `pvs`) is disk `disk` itself,
/// or a partition of `disk`. Guards against false-positive prefix matches between
/// disks that share a textual prefix, e.g. `/dev/sda` vs `/dev/sdaa1`, or
/// `/dev/nvme0n1` vs `/dev/nvme0n11p1`.
fn pv_is_on_disk(pv: &str, disk: &str) -> bool {
    if pv == disk {
        return true;
    }
    let Some(remainder) = pv.strip_prefix(disk) else {
        return false;
    };

    // Mirror common::disk_layout::partition_path's separator convention: disks
    // whose name ends in a digit (NVMe/mmc style) use a "p" separator before the
    // partition number; other disks (SATA/SCSI style) append the number directly.
    let needs_p = disk.chars().last().is_some_and(|c| c.is_ascii_digit());
    let digits = if needs_p {
        match remainder.strip_prefix('p') {
            Some(d) => d,
            None => return false,
        }
    } else {
        remainder
    };

    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// Execute a command and return error on non-zero exit.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== pv_is_on_disk tests ==========

    // Whole disk used directly as a PV should match itself exactly.
    #[test]
    fn test_pv_is_on_disk_exact_match() {
        assert!(pv_is_on_disk("/dev/sda", "/dev/sda"));
    }

    // SATA/SCSI-style disks append the partition number directly.
    #[test]
    fn test_pv_is_on_disk_sata_partition() {
        assert!(pv_is_on_disk("/dev/sda1", "/dev/sda"));
    }

    // NVMe-style disks (name ends in a digit) require a "p" separator.
    #[test]
    fn test_pv_is_on_disk_nvme_partition() {
        assert!(pv_is_on_disk("/dev/nvme0n1p1", "/dev/nvme0n1"));
    }

    // Multi-digit partition numbers must still match on both naming styles.
    #[test]
    fn test_pv_is_on_disk_multi_digit_partition_number() {
        assert!(pv_is_on_disk("/dev/sda12", "/dev/sda"));
        assert!(pv_is_on_disk("/dev/nvme0n1p12", "/dev/nvme0n1"));
    }

    // False-positive regression: /dev/sdaa1 is a distinct disk from /dev/sda,
    // not a partition of it, even though it shares the "/dev/sda" prefix.
    #[test]
    fn test_pv_is_on_disk_rejects_sibling_disk_prefix_collision() {
        assert!(!pv_is_on_disk("/dev/sdaa1", "/dev/sda"));
    }

    // False-positive regression: /dev/nvme0n11p1 (11th NVMe namespace) is a
    // distinct disk from /dev/nvme0n1, not a partition of it.
    #[test]
    fn test_pv_is_on_disk_rejects_nvme_namespace_prefix_collision() {
        assert!(!pv_is_on_disk("/dev/nvme0n11p1", "/dev/nvme0n1"));
    }

    // Unrelated disks must never match.
    #[test]
    fn test_pv_is_on_disk_rejects_unrelated_disk() {
        assert!(!pv_is_on_disk("/dev/sdb1", "/dev/sda"));
    }

    // ========== vgs_on_disks tests ==========

    #[test]
    fn test_vgs_on_disks_matches_target_disk() {
        let pvs_output = "/dev/sda1  vg_root\n/dev/sdb1  vg_data\n";
        let disks = vec!["/dev/sda".to_string()];
        let vgs = vgs_on_disks(pvs_output, &disks);
        assert_eq!(vgs, HashSet::from(["vg_root".to_string()]));
    }

    #[test]
    fn test_vgs_on_disks_empty_disks_matches_every_vg() {
        let pvs_output = "/dev/sda1  vg_root\n/dev/sdb1  vg_data\n";
        let vgs = vgs_on_disks(pvs_output, &[]);
        assert_eq!(
            vgs,
            HashSet::from(["vg_root".to_string(), "vg_data".to_string()])
        );
    }

    #[test]
    fn test_vgs_on_disks_skips_pvs_with_no_vg() {
        // A PV that isn't yet assigned to a VG has an empty second column.
        let pvs_output = "/dev/sdc1  \n";
        let disks = vec!["/dev/sdc".to_string()];
        let vgs = vgs_on_disks(pvs_output, &disks);
        assert!(vgs.is_empty());
    }

    #[test]
    fn test_vgs_on_disks_no_output() {
        let vgs = vgs_on_disks("", &[]);
        assert!(vgs.is_empty());
    }

    // ========== remove_lvm_on_disks smoke tests ==========
    //
    // These exercise the `pvs` subprocess path (which will fail or return no
    // output in a test environment) to confirm the best-effort contract:
    // no panics, regardless of whether `pvs` is installed.

    #[tokio::test]
    async fn test_remove_lvm_on_disks_empty() {
        remove_lvm_on_disks(&[]).await;
    }

    #[tokio::test]
    async fn test_remove_lvm_on_disks_with_disks() {
        remove_lvm_on_disks(&["/dev/rack-agent-test-nonexistent".to_string()]).await;
    }
}
