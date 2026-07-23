use anyhow::{Result, anyhow};
use log::{error, info};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use common::cnc::CncClient;
use common::device_attributes::DiskType;

use crate::scan::{self, WipeTarget};

/// Securely erase every whole disk on the machine.
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Enumerates every whole disk via `scan::list_wipe_targets()`
/// 3. Erases each disk using the method appropriate to its type (HDD: `shred`,
///    SATA SSD: ATA secure erase, NVMe: `nvme format`)
/// 4. Reports success or failure to rack-director
///
/// `plan_id` is forwarded to success/failure reports so rack-director can
/// discard stale reports from a previously-cancelled plan.
pub async fn wipe_disks(client: &CncClient, plan_id: Option<i64>) -> Result<()> {
    info!("Starting disk wipe...");

    let uuid = scan::read_dmi_for_uuid()
        .await?
        .ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Device UUID: {}", uuid);

    let targets = match scan::list_wipe_targets().await {
        Ok(targets) => targets,
        Err(e) => {
            let error_msg = format!("Failed to enumerate disks for wipe: {}", e);
            error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg, plan_id).await?;
            return Err(e);
        }
    };

    info!("Found {} disk(s) to wipe", targets.len());

    for target in &targets {
        if let Err(e) = wipe_disk(target).await {
            let error_msg = format!("Failed to wipe {}: {}", target.dev_node, e);
            error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg, plan_id).await?;
            return Err(e);
        }
    }

    info!("All disks wiped successfully");
    client.action_success(&uuid, plan_id).await?;
    Ok(())
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
    run_command_no_timeout(&program, &args).await
}

/// Erase a SATA SSD via ATA Secure Erase.
///
/// Fails hard (no fallback) if the drive is frozen or does not support the
/// ATA security feature set — per the erase policy, silently falling back to
/// a weaker erase method would be worse than surfacing the failure.
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
    run_command_no_timeout(&program, &args).await
}

/// Erase an NVMe disk via `nvme format`, preferring cryptographic erase.
async fn wipe_nvme(dev: &str) -> Result<()> {
    let info = run_command_capture("nvme", &["id-ctrl", dev]).await?;
    let crypto = nvme_supports_crypto_erase(&info);
    let (program, args) = wipe_argv(DiskType::Nvme, dev, false, crypto);
    run_command_no_timeout(&program, &args).await
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
/// Looks at the first substantive line of the `Security:` block (skipping the
/// `Master password revision code` line, which is present regardless of
/// support): `supported` means the feature set is available, `not supported`
/// means it is absent entirely.
fn hdparm_security_supported(hdparm_output: &str) -> bool {
    let mut in_security_block = false;
    for line in hdparm_output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Security:") {
            in_security_block = true;
            continue;
        }
        if !in_security_block {
            continue;
        }
        if trimmed.starts_with("Master password revision code") {
            continue;
        }
        if trimmed.is_empty() {
            break;
        }
        return trimmed == "supported";
    }
    false
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
/// to the log as it arrives.
///
/// `shred` and ATA secure erase can legitimately run for many hours on large
/// or slow disks, so this deliberately never times out — progress is
/// streamed to the log instead of buffering to completion.
async fn run_command_no_timeout(cmd: &str, args: &[String]) -> Result<()> {
    info!("Running: {} {}", cmd, args.join(" "));

    let mut child = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to execute {}: {}", cmd, e))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let stdout_cmd = cmd.to_string();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            info!("[{}] {}", stdout_cmd, line);
        }
    });

    let stderr_cmd = cmd.to_string();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            info!("[{} stderr] {}", stderr_cmd, line);
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
}
