//! ATA SANITIZE erase for SATA SSDs.
//!
//! SANITIZE has its own freeze lock, independent of the ATA Security feature
//! set, so it often still works on drives that firmware has security-frozen
//! at POST. It also needs no password, so a failed erase can't leave the
//! drive LOCKED.
//!
//! Output strings parsed here come from hdparm 9.65 (`identify.c`
//! `feat_word59_str`, `hdparm.c` `sanitize_normal_output`).

use std::time::Duration;

use anyhow::{Result, anyhow};
use log::info;

use super::{run_command_capture, run_command_no_timeout};

/// Interval between `--sanitize-status` polls while an operation runs.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// A SANITIZE operation the drive advertises in `hdparm -I`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SanitizeOp {
    CryptoScramble,
    BlockErase,
    Overwrite,
}

impl SanitizeOp {
    fn args(self, dev: &str) -> Vec<String> {
        let mut args = vec!["--yes-i-know-what-i-am-doing".to_string()];
        match self {
            SanitizeOp::CryptoScramble => args.push("--sanitize-crypto-scramble".to_string()),
            SanitizeOp::BlockErase => args.push("--sanitize-block-erase".to_string()),
            SanitizeOp::Overwrite => {
                args.push("--sanitize-overwrite".to_string());
                args.push("hex:00000000".to_string());
            }
        }
        args.push(dev.to_string());
        args
    }
}

/// State reported by `hdparm --sanitize-status`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SanitizeStatus {
    /// Idle; `last_succeeded` reflects the "Completed Without Error" flag.
    Idle { last_succeeded: bool },
    Frozen,
    InProgress,
}

/// Plan the SANITIZE operations to run: crypto scramble (if supported)
/// followed by a media erase — block erase, else overwrite. Empty if the
/// drive doesn't support the SANITIZE feature set or any operation.
pub(super) fn plan_ops(hdparm_info: &str) -> Vec<SanitizeOp> {
    if !has_feature(hdparm_info, "SANITIZE feature set") {
        return Vec::new();
    }
    let mut ops = Vec::new();
    if has_feature(hdparm_info, "CRYPTO_SCRAMBLE_EXT command") {
        ops.push(SanitizeOp::CryptoScramble);
    }
    if has_feature(hdparm_info, "BLOCK_ERASE_EXT command") {
        ops.push(SanitizeOp::BlockErase);
    } else if has_feature(hdparm_info, "OVERWRITE_EXT command") {
        ops.push(SanitizeOp::Overwrite);
    }
    ops
}

/// hdparm only lists word-59 sanitize features the drive supports (prefixed
/// with `*`), so a line naming the feature exactly means supported.
fn has_feature(hdparm_info: &str, feature: &str) -> bool {
    hdparm_info.lines().any(|line| {
        let line = line.trim();
        line.trim_start_matches('*').trim() == feature
    })
}

/// Parse `hdparm --sanitize-status` stdout.
pub(super) fn parse_status(output: &str) -> Result<SanitizeStatus> {
    let state = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("State:"))
        .map(str::trim)
        .ok_or_else(|| anyhow!("no State line in sanitize status output"))?;

    if state.starts_with("SD1") {
        Ok(SanitizeStatus::Frozen)
    } else if state.starts_with("SD2") {
        Ok(SanitizeStatus::InProgress)
    } else if state.starts_with("SD0") || state.starts_with("SD4") {
        let last_succeeded = output
            .lines()
            .any(|l| l.trim() == "Last Sanitize Operation Completed Without Error");
        Ok(SanitizeStatus::Idle { last_succeeded })
    } else {
        Err(anyhow!("unexpected sanitize state: {}", state))
    }
}

/// Query the drive's current sanitize status.
///
/// hdparm exits non-zero when the drive aborts SANITIZE STATUS, which is how
/// a failed previous operation is reported (error reason on stderr).
pub(super) async fn query_status(dev: &str) -> Result<SanitizeStatus> {
    let output = run_command_capture("hdparm", &["--sanitize-status", dev]).await?;
    parse_status(&output)
}

/// Run each planned op in order, waiting for each to complete successfully.
///
/// SANITIZE runs in the background on the drive (hdparm returns at once), so
/// completion is detected by polling. There is no timeout, matching the
/// other erase methods: block erase/overwrite can legitimately take hours.
///
/// An operation already in progress (e.g. from an earlier attempt interrupted
/// by an agent reboot — SANITIZE survives resets) is waited out first, since
/// the drive aborts new SANITIZE commands until it finishes.
pub(super) async fn run(dev: &str, ops: &[SanitizeOp]) -> Result<()> {
    if matches!(query_status(dev).await, Ok(SanitizeStatus::InProgress)) {
        info!("[{}] Waiting for in-progress SANITIZE to finish", dev);
        while matches!(query_status(dev).await, Ok(SanitizeStatus::InProgress)) {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
    for &op in ops {
        info!("[{}] Starting SANITIZE {:?}", dev, op);
        run_command_no_timeout(dev, "hdparm", &op.args(dev)).await?;
        wait_for_completion(dev, op).await?;
        info!("[{}] SANITIZE {:?} completed", dev, op);
    }
    Ok(())
}

async fn wait_for_completion(dev: &str, op: SanitizeOp) -> Result<()> {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let status = query_status(dev)
            .await
            .map_err(|e| anyhow!("SANITIZE {:?} failed: {}", op, e))?;
        match status {
            SanitizeStatus::InProgress => continue,
            SanitizeStatus::Idle { last_succeeded: true } => return Ok(()),
            SanitizeStatus::Idle { last_succeeded: false } => {
                return Err(anyhow!(
                    "SANITIZE {:?} finished without reporting success",
                    op
                ));
            }
            SanitizeStatus::Frozen => {
                return Err(anyhow!("drive became sanitize-frozen during {:?}", op));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature lines as printed for a Samsung 883 DCT (security-frozen at POST).
    const INFO_883_DCT: &str = "Commands/features:
\tEnabled\tSupported:
\t   *\tSCT Data Tables (AC5)
\t   *\tSANITIZE_ANTIFREEZE_LOCK_EXT command
\t   *\tSANITIZE feature set
\t   *\tCRYPTO_SCRAMBLE_EXT command
\t   *\tBLOCK_ERASE_EXT command
\t   *\tDevice encrypts all user data
";

    #[test]
    fn test_plan_ops_crypto_then_block() {
        assert_eq!(
            plan_ops(INFO_883_DCT),
            vec![SanitizeOp::CryptoScramble, SanitizeOp::BlockErase]
        );
    }

    #[test]
    fn test_plan_ops_overwrite_when_no_block_erase() {
        let info = "\t   *\tSANITIZE feature set\n\t   *\tOVERWRITE_EXT command\n";
        assert_eq!(plan_ops(info), vec![SanitizeOp::Overwrite]);
    }

    #[test]
    fn test_plan_ops_empty_without_feature_set() {
        // Operation lines without the feature-set line must not be trusted.
        let info = "\t   *\tBLOCK_ERASE_EXT command\n";
        assert!(plan_ops(info).is_empty());
    }

    #[test]
    fn test_plan_ops_ignores_antifreeze_line() {
        let info = "\t   *\tSANITIZE_ANTIFREEZE_LOCK_EXT command\n";
        assert!(plan_ops(info).is_empty());
    }

    #[test]
    fn test_parse_status_idle() {
        let out = "\n/dev/sda:\nIssuing SANITIZE_STATUS command\nSanitize status:\n    State:    SD0 Sanitize Idle\n";
        assert_eq!(
            parse_status(out).unwrap(),
            SanitizeStatus::Idle { last_succeeded: false }
        );
    }

    #[test]
    fn test_parse_status_idle_succeeded() {
        let out = "Sanitize status:\n    State:    SD0 Sanitize Idle\n    Last Sanitize Operation Completed Without Error\n";
        assert_eq!(
            parse_status(out).unwrap(),
            SanitizeStatus::Idle { last_succeeded: true }
        );
    }

    #[test]
    fn test_parse_status_frozen() {
        let out = "Sanitize status:\n    State:    SD1 Sanitize Frozen\n";
        assert_eq!(parse_status(out).unwrap(), SanitizeStatus::Frozen);
    }

    #[test]
    fn test_parse_status_in_progress() {
        let out = "Sanitize status:\n    State:    SD2 Sanitize operation In Process\n    Progress: 0x1fff (12%)\n";
        assert_eq!(parse_status(out).unwrap(), SanitizeStatus::InProgress);
    }

    #[test]
    fn test_parse_status_missing_state_is_error() {
        assert!(parse_status("Sanitize status:\n").is_err());
    }

    #[test]
    fn test_op_args() {
        assert_eq!(
            SanitizeOp::Overwrite.args("/dev/sda"),
            vec![
                "--yes-i-know-what-i-am-doing",
                "--sanitize-overwrite",
                "hex:00000000",
                "/dev/sda"
            ]
        );
        assert_eq!(
            SanitizeOp::CryptoScramble.args("/dev/sda"),
            vec![
                "--yes-i-know-what-i-am-doing",
                "--sanitize-crypto-scramble",
                "/dev/sda"
            ]
        );
    }
}
