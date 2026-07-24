use anyhow::{Result, anyhow};
use log::{error, info, warn};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use common::cnc::CncClient;
use common::device_attributes::DiskType;

use crate::scan::{self, WipeTarget};

/// Securely erase every whole disk on the machine.
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Enumerates every whole disk via `scan::list_wipe_targets()`, and fails
///    if that enumeration found zero disks or missed a real one (see
///    `validate_wipe_targets`)
/// 3. Deactivates any LVM volume groups on the machine so they can't hold a
///    disk busy during erase
/// 4. Erases each disk concurrently using the method appropriate to its type
///    (HDD: `shred`, SATA SSD: ATA secure erase, NVMe: `nvme format`)
/// 5. Reports success or failure to rack-director
///
/// `plan_id` is forwarded to success/failure reports so rack-director can
/// discard stale reports from a previously-cancelled plan.
pub async fn wipe_disks(client: &CncClient, plan_id: Option<i64>) -> Result<()> {
    info!("Starting disk wipe...");

    let uuid = scan::read_dmi_for_uuid()
        .await?
        .ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Device UUID: {}", uuid);

    let targets = match discover_and_validate_wipe_targets().await {
        Ok(targets) => targets,
        Err(e) => {
            let error_msg = format!("Failed to enumerate disks for wipe: {}", e);
            error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg, plan_id).await?;
            return Err(e);
        }
    };

    info!("Found {} disk(s) to wipe", targets.len());

    // Step 0: best-effort LVM/dm cleanup so a residual VG/PV doesn't hold a
    // disk busy (nvme format/hdparm secure-erase both refuse to run against a
    // disk with an open handle). wipe_disks always targets every whole disk
    // on the machine, so — mirroring partition.rs's wipe_all_disks path — an
    // empty slice is passed to remove every VG on the system.
    crate::lvm::remove_lvm_on_disks(&[]).await;

    if let Err(e) = wipe_all_targets(targets).await {
        let error_msg = e.to_string();
        error!("{}", error_msg);
        client.action_failed(&uuid, &error_msg, plan_id).await?;
        return Err(e);
    }

    info!("All disks wiped successfully");
    client.action_success(&uuid, plan_id).await?;
    Ok(())
}

// ========== Target Discovery ==========

/// Enumerate wipe targets, failing loudly rather than silently wiping fewer
/// disks than actually exist on the machine.
async fn discover_and_validate_wipe_targets() -> Result<Vec<WipeTarget>> {
    let targets = scan::list_wipe_targets().await?;
    let real_devices = scan::list_real_block_device_names().await?;
    validate_wipe_targets(targets, &real_devices)
}

/// Validate that `targets` (from `/dev/disk/by-path` enumeration) is
/// non-empty and covers every real whole disk in `real_devices` (from
/// `/sys/block`).
///
/// `by-path` enumeration can silently miss real disks — e.g. systemd >= 254
/// does not create `by-path` symlinks for `mmcblk*` devices (systemd issue
/// #28671) — so a device could otherwise go completely unwiped with the
/// action still reporting success. Both a total miss (zero targets) and a
/// partial miss (some targets found, one or more real disks missing from
/// them) are treated as fatal errors that name the affected device(s).
fn validate_wipe_targets(
    targets: Vec<WipeTarget>,
    real_devices: &[String],
) -> Result<Vec<WipeTarget>> {
    if targets.is_empty() {
        return Err(anyhow!(
            "No disks found to wipe; refusing to report success having wiped nothing"
        ));
    }

    let found: std::collections::HashSet<&str> = targets.iter().map(|t| t.name.as_str()).collect();
    let missing: Vec<&str> = real_devices
        .iter()
        .map(|s| s.as_str())
        .filter(|name| !found.contains(name))
        .collect();

    if !missing.is_empty() {
        return Err(anyhow!(
            "{} disk(s) present in /sys/block were missed by by-path enumeration and \
             would be left unwiped: {}",
            missing.len(),
            missing.join(", ")
        ));
    }

    Ok(targets)
}

// ========== Concurrent Wipe Execution ==========

/// Wipe every target disk concurrently, attempting all of them regardless of
/// individual failures.
///
/// Disks are independent block devices with no shared kernel state between
/// their `shred`/`hdparm`/`nvme` invocations, so wiping them concurrently is
/// safe and avoids an ~Nx slowdown on a chassis with many bays. A failure on
/// one disk does not stop the others: every disk is given a chance to
/// complete before an aggregate error (naming every failed disk) is
/// returned — a failure on disk 2 of 12 must not leave disks 3-12 untouched,
/// especially since a retry would otherwise restart from disk 1 and each
/// pass can take hours.
async fn wipe_all_targets(targets: Vec<WipeTarget>) -> Result<()> {
    let mut set = tokio::task::JoinSet::new();
    for target in targets {
        set.spawn(async move {
            let result = wipe_disk(&target).await;
            (target.dev_node, result)
        });
    }

    let mut outcomes = Vec::with_capacity(set.len());
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(outcome) => outcomes.push(outcome),
            Err(join_err) => outcomes.push((
                "<unknown disk>".to_string(),
                Err(anyhow!("wipe task panicked: {}", join_err)),
            )),
        }
    }

    aggregate_wipe_results(outcomes)
}

/// Turn per-disk wipe outcomes into a single `Result`, succeeding only if
/// every disk succeeded. On failure, the error names every disk that failed
/// and why, so a partial-batch failure is never reported as an opaque
/// single-disk error.
fn aggregate_wipe_results(outcomes: Vec<(String, Result<()>)>) -> Result<()> {
    let total = outcomes.len();
    let failures: Vec<String> = outcomes
        .into_iter()
        .filter_map(|(dev_node, result)| match result {
            Ok(()) => None,
            Err(e) => {
                error!("Failed to wipe {}: {}", dev_node, e);
                Some(format!("{}: {}", dev_node, e))
            }
        })
        .collect();

    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "{} of {} disk(s) failed to wipe: {}",
            failures.len(),
            total,
            failures.join("; ")
        ))
    }
}

/// Erase a single disk using the method appropriate to its detected type.
async fn wipe_disk(target: &WipeTarget) -> Result<()> {
    info!(
        "Wiping {} ({}, {:?})",
        target.dev_node, target.name, target.disk_type
    );
    match target.disk_type {
        DiskType::Hdd => wipe_hdd(&target.dev_node).await,
        DiskType::Ssd => wipe_ssd(&target.dev_node).await,
        DiskType::Nvme => wipe_nvme(&target.dev_node).await,
    }
}

/// Erase an HDD with a single-pass random overwrite (`shred -v -n1`).
async fn wipe_hdd(dev: &str) -> Result<()> {
    let (program, args) = wipe_argv(DiskType::Hdd, dev, false, false);
    run_command_no_timeout(dev, &program, &args).await
}

/// Erase a SATA SSD via ATA Secure Erase.
///
/// Fails hard (no fallback) if the drive is frozen or does not support the
/// ATA security feature set — per the erase policy, silently falling back to
/// a weaker erase method would be worse than surfacing the failure.
///
/// If the erase command itself fails after the user password has been set,
/// the drive would be left LOCKED on next power cycle (per kernel.org's ATA
/// Secure Erase documentation) unless the password is disabled again — see
/// `disable_security_password_best_effort`.
async fn wipe_ssd(dev: &str) -> Result<()> {
    let info = run_command_capture("hdparm", &["-I", dev]).await?;

    if hdparm_is_frozen(&info) {
        return Err(anyhow!(
            "disk is security-frozen; refusing to erase (a power cycle is required to unfreeze it)"
        ));
    }
    if !hdparm_security_supported(&info) {
        return Err(anyhow!(
            "disk does not support the ATA security feature set"
        ));
    }

    run_command_no_timeout(
        dev,
        "hdparm",
        &[
            "--user-master".to_string(),
            "u".to_string(),
            "--security-set-pass".to_string(),
            "p".to_string(),
            dev.to_string(),
        ],
    )
    .await?;

    let enhanced = hdparm_supports_enhanced(&info);
    let (program, args) = wipe_argv(DiskType::Ssd, dev, enhanced, false);
    match run_command_no_timeout(dev, &program, &args).await {
        Ok(()) => Ok(()),
        Err(erase_err) => Err(disable_security_password_best_effort(dev, erase_err).await),
    }
}

/// After a security-erase failure, best-effort disable the drive's user
/// security password (armed just before the erase attempt) so it isn't left
/// LOCKED on next power cycle.
///
/// Always returns the ORIGINAL erase error: the disable attempt's own
/// outcome is only logged, never returned in its place — a failed erase must
/// never be reported as a success, and a failed disable must never mask the
/// erase failure that caused it.
async fn disable_security_password_best_effort(
    dev: &str,
    erase_err: anyhow::Error,
) -> anyhow::Error {
    let disable_args = vec![
        "--user-master".to_string(),
        "u".to_string(),
        "--security-disable".to_string(),
        "p".to_string(),
        dev.to_string(),
    ];
    let disable_result = run_command_no_timeout(dev, "hdparm", &disable_args).await;
    log_disable_outcome(dev, &erase_err, disable_result);
    erase_err
}

/// Log the outcome of the best-effort security-disable attempt. Never
/// changes what gets returned to the caller of `disable_security_password_best_effort`.
fn log_disable_outcome(dev: &str, erase_err: &anyhow::Error, disable_result: Result<()>) {
    if let Err(disable_err) = disable_result {
        warn!(
            "hdparm --security-disable failed on {} after erase failure; drive may be \
             LOCKED on next power cycle. erase error: {}; disable error: {}",
            dev, erase_err, disable_err
        );
    }
}

/// Erase an NVMe disk via `nvme format`, preferring cryptographic erase.
async fn wipe_nvme(dev: &str) -> Result<()> {
    let info = run_command_capture("nvme", &["id-ctrl", dev]).await?;
    let crypto = nvme_supports_crypto_erase(&info);
    let (program, args) = wipe_argv(DiskType::Nvme, dev, false, crypto);
    run_command_no_timeout(dev, &program, &args).await
}

// ========== Command Argument Selection ==========

/// Build the erase command and arguments for a disk, branching on type.
///
/// `enhanced` selects `--security-erase-enhanced` over `--security-erase` for
/// SATA SSDs; `crypto` selects `--ses=2` (crypto erase) over `--ses=1` (user
/// data erase) for NVMe. Both flags are ignored for the disk types they don't
/// apply to.
fn wipe_argv(
    disk_type: DiskType,
    dev: &str,
    enhanced: bool,
    crypto: bool,
) -> (String, Vec<String>) {
    match disk_type {
        DiskType::Hdd => (
            "shred".to_string(),
            vec!["-v".to_string(), "-n1".to_string(), dev.to_string()],
        ),
        DiskType::Ssd => {
            let erase_flag = if enhanced {
                "--security-erase-enhanced"
            } else {
                "--security-erase"
            };
            (
                "hdparm".to_string(),
                vec![
                    "--user-master".to_string(),
                    "u".to_string(),
                    erase_flag.to_string(),
                    "p".to_string(),
                    dev.to_string(),
                ],
            )
        }
        DiskType::Nvme => {
            let ses = if crypto { "--ses=2" } else { "--ses=1" };
            (
                "nvme".to_string(),
                vec![
                    "format".to_string(),
                    dev.to_string(),
                    ses.to_string(),
                    "--force".to_string(),
                ],
            )
        }
    }
}

// ========== hdparm -I Parsing ==========

/// Returns true if `hdparm -I` output shows the drive's security state as
/// `frozen` (i.e. the "frozen" line does NOT have a "not" qualifier).
///
/// Matches on the last whitespace-separated token of the line reporting
/// frozen state so it is robust to hdparm's tab-vs-space indentation.
fn hdparm_is_frozen(hdparm_output: &str) -> bool {
    hdparm_output
        .lines()
        .find(|line| line.split_whitespace().next_back() == Some("frozen"))
        .map(|line| !line.contains("not"))
        .unwrap_or(false)
}

/// Returns true if `hdparm -I` output shows enhanced erase as supported
/// (a line reading exactly `supported: enhanced erase`).
fn hdparm_supports_enhanced(hdparm_output: &str) -> bool {
    hdparm_output
        .lines()
        .any(|line| line.trim() == "supported: enhanced erase")
}

/// Returns true if the drive supports the ATA security feature set at all.
///
/// Scans the `Security:` block (matching the sibling parsers' whole-block
/// scan style, rather than trusting a fixed line offset within it) for a
/// line that trims to exactly `supported`. Absence of the block, or absence
/// of that line within it (e.g. `not supported`), both mean the feature is
/// unsupported.
fn hdparm_security_supported(hdparm_output: &str) -> bool {
    security_block_lines(hdparm_output).any(|line| line.trim() == "supported")
}

/// Return the lines of the `Security:` block in `hdparm -I` output, i.e. the
/// lines strictly between a `Security:` header line and the next blank line
/// (or end of output). Yields nothing if no `Security:` header is present.
fn security_block_lines(hdparm_output: &str) -> impl Iterator<Item = &str> {
    hdparm_output
        .lines()
        .skip_while(|line| !line.trim().starts_with("Security:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
}

// ========== nvme id-ctrl Parsing ==========

/// Returns true if the `fna` (Format NVM Attributes) field in `nvme id-ctrl`
/// output has bit 2 (`0x4`, Cryptographic Erase Supported) set.
fn nvme_supports_crypto_erase(id_ctrl_output: &str) -> bool {
    for line in id_ctrl_output.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("fna") else {
            continue;
        };
        // Guard against accidentally matching a different field whose name
        // merely starts with "fna" (none exist today, but keep this safe).
        if !rest.starts_with(|c: char| c.is_whitespace() || c == ':') {
            continue;
        }
        let Some((_, value)) = rest.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let value = value.split_whitespace().next().unwrap_or(value);
        let parsed = value
            .strip_prefix("0x")
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| value.parse::<u32>().ok());
        if let Some(v) = parsed {
            return v & 0x4 != 0;
        }
    }
    false
}

// ========== Command Execution ==========

/// Run a short, quick-completing command and capture its stdout as text.
///
/// Used for informational queries (`hdparm -I`, `nvme id-ctrl`) that
/// complete near-instantly, as opposed to the long-running erase commands.
async fn run_command_capture(cmd: &str, args: &[&str]) -> Result<String> {
    info!("Running: {} {}", cmd, args.join(" "));

    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute {}: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "{} failed (exit {}): {}",
            cmd,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run a command to completion with no timeout, streaming its stdout/stderr
/// to the log as it arrives, tagged with `log_tag` (the device node) so
/// output from concurrently-wiped disks stays attributable when interleaved.
///
/// `shred` and ATA secure erase can legitimately run for many hours on large
/// or slow disks, so this deliberately never times out. `kill_on_drop` is set
/// purely as insurance against an orphaned erase process if this future is
/// ever dropped before completion (e.g. a panic elsewhere in the join set) —
/// it does not introduce any wall-clock or idle-output timeout.
async fn run_command_no_timeout(log_tag: &str, cmd: &str, args: &[String]) -> Result<()> {
    info!("[{}] Running: {} {}", log_tag, cmd, args.join(" "));

    let mut child = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("Failed to execute {}: {}", cmd, e))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let stdout_tag = format!("{} {}", log_tag, cmd);
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            info!("[{}] {}", stdout_tag, line);
        }
    });

    let stderr_tag = format!("{} {}", log_tag, cmd);
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            info!("[{} stderr] {}", stderr_tag, line);
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| anyhow!("Failed waiting for {}: {}", cmd, e))?;

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    if !status.success() {
        return Err(anyhow!(
            "{} failed (exit {})",
            cmd,
            status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== wipe_argv tests ==========

    #[test]
    fn test_wipe_argv_hdd() {
        let (program, args) = wipe_argv(DiskType::Hdd, "/dev/sda", false, false);
        assert_eq!(program, "shred");
        assert_eq!(args, vec!["-v", "-n1", "/dev/sda"]);
    }

    #[test]
    fn test_wipe_argv_ssd_non_enhanced() {
        let (program, args) = wipe_argv(DiskType::Ssd, "/dev/sdb", false, false);
        assert_eq!(program, "hdparm");
        assert_eq!(
            args,
            vec!["--user-master", "u", "--security-erase", "p", "/dev/sdb"]
        );
    }

    #[test]
    fn test_wipe_argv_ssd_enhanced() {
        let (program, args) = wipe_argv(DiskType::Ssd, "/dev/sdb", true, false);
        assert_eq!(program, "hdparm");
        assert_eq!(
            args,
            vec![
                "--user-master",
                "u",
                "--security-erase-enhanced",
                "p",
                "/dev/sdb"
            ]
        );
    }

    #[test]
    fn test_wipe_argv_nvme_ses1() {
        let (program, args) = wipe_argv(DiskType::Nvme, "/dev/nvme0n1", false, false);
        assert_eq!(program, "nvme");
        assert_eq!(args, vec!["format", "/dev/nvme0n1", "--ses=1", "--force"]);
    }

    #[test]
    fn test_wipe_argv_nvme_ses2() {
        let (program, args) = wipe_argv(DiskType::Nvme, "/dev/nvme0n1", false, true);
        assert_eq!(program, "nvme");
        assert_eq!(args, vec!["format", "/dev/nvme0n1", "--ses=2", "--force"]);
    }

    // ========== hdparm -I fixtures ==========

    const HDPARM_NOT_FROZEN_ENHANCED: &str = r#"/dev/sda:

ATA device, with non-removable media
	Model Number:       Samsung SSD 860 EVO 500GB
	Serial Number:      S3Z2NB0K123456
	Firmware Revision:  RVT01B6Q
Security:
	Master password revision code = 65534
		supported
	not	enabled
	not	locked
	not	frozen
	not	expired: security count
		supported: enhanced erase
	2min for SECURITY ERASE UNIT. 2min for ENHANCED SECURITY ERASE UNIT.
"#;

    const HDPARM_FROZEN: &str = r#"/dev/sda:

ATA device, with non-removable media
	Model Number:       Samsung SSD 860 EVO 500GB
Security:
	Master password revision code = 65534
		supported
	not	enabled
	not	locked
		frozen
	not	expired: security count
		supported: enhanced erase
"#;

    const HDPARM_NOT_SUPPORTED: &str = r#"/dev/sda:

ATA device, with non-removable media
	Model Number:       Old Spinning Disk
Security:
	Master password revision code = 65534
	not	supported
"#;

    const HDPARM_SUPPORTED_NO_ENHANCED: &str = r#"/dev/sda:

ATA device, with non-removable media
Security:
	Master password revision code = 65534
		supported
	not	enabled
	not	locked
	not	frozen
	not	expired: security count
	2min for SECURITY ERASE UNIT.
"#;

    // ========== hdparm_is_frozen tests ==========

    #[test]
    fn test_hdparm_is_frozen_false_when_not_frozen() {
        assert!(!hdparm_is_frozen(HDPARM_NOT_FROZEN_ENHANCED));
    }

    #[test]
    fn test_hdparm_is_frozen_true_when_frozen() {
        assert!(hdparm_is_frozen(HDPARM_FROZEN));
    }

    #[test]
    fn test_hdparm_is_frozen_false_when_security_not_supported() {
        assert!(!hdparm_is_frozen(HDPARM_NOT_SUPPORTED));
    }

    // ========== hdparm_supports_enhanced tests ==========

    #[test]
    fn test_hdparm_supports_enhanced_true() {
        assert!(hdparm_supports_enhanced(HDPARM_NOT_FROZEN_ENHANCED));
    }

    #[test]
    fn test_hdparm_supports_enhanced_false_when_absent() {
        assert!(!hdparm_supports_enhanced(HDPARM_SUPPORTED_NO_ENHANCED));
    }

    #[test]
    fn test_hdparm_supports_enhanced_false_when_not_supported() {
        assert!(!hdparm_supports_enhanced(HDPARM_NOT_SUPPORTED));
    }

    // ========== hdparm_security_supported tests ==========

    #[test]
    fn test_hdparm_security_supported_true() {
        assert!(hdparm_security_supported(HDPARM_NOT_FROZEN_ENHANCED));
    }

    #[test]
    fn test_hdparm_security_supported_false() {
        assert!(!hdparm_security_supported(HDPARM_NOT_SUPPORTED));
    }

    #[test]
    fn test_hdparm_security_supported_false_when_no_security_block() {
        let output = "/dev/sda:\n\nATA device, with non-removable media\n";
        assert!(!hdparm_security_supported(output));
    }

    #[test]
    fn test_hdparm_security_supported_true_when_frozen() {
        // Security support and frozen state are independent; a frozen drive
        // can still report the feature set as supported.
        assert!(hdparm_security_supported(HDPARM_FROZEN));
    }

    // ========== nvme_supports_crypto_erase fixtures + tests ==========

    const NVME_ID_CTRL_CRYPTO_SUPPORTED: &str = r#"NVME Identify Controller:
vid       : 0x144d
ssvid     : 0x144d
sn        : S4EWNX0M123456
mn        : Samsung SSD 970 EVO 1TB
fr        : 2B2QEXM7
fna       : 0x4
nn        : 1
"#;

    const NVME_ID_CTRL_CRYPTO_NOT_SUPPORTED: &str = r#"NVME Identify Controller:
vid       : 0x1b36
ssvid     : 0x1af4
sn        : deadbeef
mn        : QEMU NVMe Ctrl
fr        : 1.0
fna       : 0x0
nn        : 1
"#;

    const NVME_ID_CTRL_MULTI_BIT_FNA: &str = r#"NVME Identify Controller:
mn        : Multi Bit Drive
fna       : 0x6
"#;

    #[test]
    fn test_nvme_supports_crypto_erase_true() {
        assert!(nvme_supports_crypto_erase(NVME_ID_CTRL_CRYPTO_SUPPORTED));
    }

    #[test]
    fn test_nvme_supports_crypto_erase_false() {
        assert!(!nvme_supports_crypto_erase(
            NVME_ID_CTRL_CRYPTO_NOT_SUPPORTED
        ));
    }

    #[test]
    fn test_nvme_supports_crypto_erase_true_when_other_bits_also_set() {
        assert!(nvme_supports_crypto_erase(NVME_ID_CTRL_MULTI_BIT_FNA));
    }

    #[test]
    fn test_nvme_supports_crypto_erase_false_when_field_absent() {
        let output = "NVME Identify Controller:\nvid       : 0x144d\n";
        assert!(!nvme_supports_crypto_erase(output));
    }

    // ========== validate_wipe_targets tests ==========

    fn make_target(name: &str, disk_type: DiskType) -> WipeTarget {
        WipeTarget {
            dev_node: format!("/dev/{}", name),
            name: name.to_string(),
            disk_type,
        }
    }

    #[test]
    fn test_validate_wipe_targets_fails_when_empty() {
        let err = validate_wipe_targets(vec![], &[]).unwrap_err();
        assert!(err.to_string().contains("No disks found to wipe"));
    }

    #[test]
    fn test_validate_wipe_targets_passes_when_every_real_disk_covered() {
        let targets = vec![
            make_target("sda", DiskType::Hdd),
            make_target("nvme0n1", DiskType::Nvme),
        ];
        let real_devices = vec!["sda".to_string(), "nvme0n1".to_string()];

        let result = validate_wipe_targets(targets, &real_devices).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_validate_wipe_targets_fails_and_names_missing_disk() {
        let targets = vec![make_target("sda", DiskType::Hdd)];
        // mmcblk0 exists in /sys/block but has no by-path symlink (systemd#28671).
        let real_devices = vec!["sda".to_string(), "mmcblk0".to_string()];

        let err = validate_wipe_targets(targets, &real_devices).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mmcblk0"));
        assert!(!msg.contains("No disks found"));
    }

    // ========== aggregate_wipe_results tests ==========

    #[test]
    fn test_aggregate_wipe_results_all_success() {
        let outcomes = vec![
            ("/dev/sda".to_string(), Ok(())),
            ("/dev/sdb".to_string(), Ok(())),
        ];
        assert!(aggregate_wipe_results(outcomes).is_ok());
    }

    #[test]
    fn test_aggregate_wipe_results_names_every_failed_disk() {
        let outcomes = vec![
            ("/dev/sda".to_string(), Ok(())),
            (
                "/dev/sdb".to_string(),
                Err(anyhow!("shred failed (exit 1)")),
            ),
            (
                "/dev/nvme0n1".to_string(),
                Err(anyhow!("nvme format failed (exit 1)")),
            ),
        ];

        let err = aggregate_wipe_results(outcomes).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/dev/sdb"));
        assert!(msg.contains("/dev/nvme0n1"));
        assert!(!msg.contains("/dev/sda: "));
        assert!(msg.contains("2 of"));
    }

    // ========== disable_security_password_best_effort tests ==========

    /// Even when the best-effort security-disable itself fails (as it always
    /// will here — `/dev/rack-agent-test-nonexistent` does not exist), the
    /// ORIGINAL erase error must be what's returned, never the disable error.
    #[tokio::test]
    async fn test_disable_security_password_best_effort_returns_original_error() {
        let erase_err = anyhow!("hdparm failed (exit 1)");
        let dev = "/dev/rack-agent-test-nonexistent";

        let returned = disable_security_password_best_effort(dev, erase_err).await;

        assert!(returned.to_string().contains("hdparm failed (exit 1)"));
    }
}
