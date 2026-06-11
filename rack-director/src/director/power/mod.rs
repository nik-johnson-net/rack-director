//! Power management abstraction for rack-director.
//!
//! This module provides a protocol-agnostic `PowerDriver` trait with two
//! implementations: Redfish (HTTP, preferred) and IPMI (ipmitool, fallback).
//!
//! The primary entry point for callers is [`resolve_power_driver`], which probes
//! the BMC and returns the best available driver.  `Director::power_driver_for`
//! wraps it with the BMC credential lookup.

mod ipmi;
mod redfish;

pub use ipmi::IpmiDriver;

use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};

/// Window within which a `last_polled_at` timestamp is considered "in daemon
/// mode".  Set to ~3× the agent's 5-second poll interval so a brief network
/// hiccup does not cause an unnecessary power kick.
pub const DAEMON_HEARTBEAT_WINDOW: Duration = Duration::from_secs(15);

/// Power-state configuration shared across all power operations.
///
/// `PowerConfig` is `Copy` so it can be stored on `Director` without lifetime
/// entanglement.
#[derive(Debug, Clone, Copy)]
pub struct PowerConfig {
    /// Whether to verify the BMC's TLS certificate for Redfish connections.
    ///
    /// Defaults to `false` because most BMC firmware ships with self-signed
    /// certificates.  Set to `true` in production environments with
    /// properly-signed BMC certificates (via `--redfish-verify-tls`).
    pub verify_tls: bool,
    /// Timeout for individual HTTP requests to the BMC.
    pub http_timeout: Duration,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            verify_tls: false,
            http_timeout: Duration::from_secs(4),
        }
    }
}

/// Observed power state of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    On,
    Off,
    Unknown,
}

/// Protocol-agnostic interface for OOB power management of a single device.
#[async_trait::async_trait]
pub trait PowerDriver: Send + Sync {
    /// Query the current power state of the device.
    async fn power_state(&self) -> anyhow::Result<PowerState>;

    /// Power the device on.
    async fn power_on(&self) -> anyhow::Result<()>;

    /// Power the device off.
    ///
    /// When `graceful` is `true` the driver requests a graceful OS shutdown;
    /// when `false` it issues an immediate (hard) power-off.
    async fn power_off(&self, graceful: bool) -> anyhow::Result<()>;

    /// Power-cycle the device (off then on).
    async fn power_cycle(&self) -> anyhow::Result<()>;

    /// Issue a hardware reset (equivalent to the front-panel reset button).
    async fn power_reset(&self) -> anyhow::Result<()>;

    /// Short string identifying the driver variant for logging and API
    /// provenance (`"redfish"` or `"ipmi"`).
    fn kind(&self) -> &'static str;
}

/// Returns `true` if `last_polled_at` falls within `window` of the current
/// UTC time, indicating the agent is actively polling in daemon mode.
///
/// A return value of `true` means the agent will pick up plan changes on its
/// next poll cycle and an OOB power kick is unnecessary.
///
/// # Parsing rules
///
/// The column value comes from SQLite and may be in one of two formats:
/// - RFC 3339 / ISO 8601 (e.g. `"2026-06-10T12:34:56Z"`)
/// - SQLite `CURRENT_TIMESTAMP` space-separated UTC (e.g. `"2026-06-10 12:34:56"`)
///
/// Any value that cannot be parsed, is `None`, is the default sentinel `"0"`,
/// or refers to a timestamp in the future, is treated as **not** in daemon mode
/// (the safe default: proceed with the kick).
pub fn is_in_daemon_mode(last_polled_at: Option<&str>, window: Duration) -> bool {
    let ts = match last_polled_at {
        None => return false,
        Some("") | Some("0") => return false,
        Some(s) => s,
    };

    let dt = parse_timestamp(ts);
    match dt {
        None => false,
        Some(t) => {
            let now = Utc::now();
            // Reject future timestamps (clock skew / corrupt data) — treat as not daemon.
            if t > now {
                return false;
            }
            let age = now.signed_duration_since(t);
            match age.to_std() {
                Ok(age_std) => age_std <= window,
                Err(_) => false,
            }
        }
    }
}

/// Try to parse `s` as either RFC 3339 or the SQLite `"%Y-%m-%d %H:%M:%S"` format
/// (treated as UTC).  Returns `None` on failure.
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Try RFC 3339 first (e.g. "2026-06-10T12:34:56Z" or with offset)
    if let Ok(dt) = s.parse::<DateTime<Utc>>() {
        return Some(dt);
    }
    // chrono's DateTime<FixedOffset> covers RFC 3339 with non-UTC offsets
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // SQLite CURRENT_TIMESTAMP: "2026-06-10 12:34:56" — treat as UTC
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }
    None
}

/// Probe the BMC and return the best available `PowerDriver`.
///
/// Attempts a Redfish probe (`GET https://{ip}/redfish/v1/`) with the
/// configured timeout.  On success, discovers the ComputerSystem path and
/// returns a `RedfishDriver`.  On any failure (connection refused, timeout,
/// non-2xx response) falls back to `IpmiDriver`.
///
/// Returns `Some(driver)` in both the Redfish and IPMI cases — the only
/// failure is an internal error building the reqwest client, which is
/// logged and results in `None`.
///
/// # Arguments
///
/// * `ip`       – BMC IP address (no scheme)
/// * `username` – BMC credential username
/// * `password` – BMC credential password
/// * `config`   – Timeout and TLS verification settings
pub async fn resolve_power_driver(
    ip: &str,
    username: &str,
    password: &str,
    config: PowerConfig,
) -> Option<Box<dyn PowerDriver>> {
    // Try Redfish first
    match redfish::RedfishDriver::discover(ip, username, password, config).await {
        Ok(driver) => {
            log::debug!("BMC at {} supports Redfish; using Redfish driver", ip);
            Some(Box::new(driver))
        }
        Err(e) => {
            log::debug!(
                "Redfish probe failed for {} ({}); falling back to IPMI",
                ip,
                e
            );
            Some(Box::new(IpmiDriver::new(
                ip.to_string(),
                username.to_string(),
                password.to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // -----------------------------------------------------------------------
    // is_in_daemon_mode tests
    // -----------------------------------------------------------------------

    fn now_rfc3339() -> String {
        Utc::now().to_rfc3339()
    }

    fn now_sqlite() -> String {
        Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn seconds_ago_rfc3339(secs: i64) -> String {
        (Utc::now() - chrono::Duration::seconds(secs)).to_rfc3339()
    }

    fn seconds_ago_sqlite(secs: i64) -> String {
        (Utc::now() - chrono::Duration::seconds(secs))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }

    fn seconds_from_now_rfc3339(secs: i64) -> String {
        (Utc::now() + chrono::Duration::seconds(secs)).to_rfc3339()
    }

    const WINDOW: Duration = Duration::from_secs(15);

    #[test]
    fn test_none_is_not_daemon() {
        assert!(!is_in_daemon_mode(None, WINDOW));
    }

    #[test]
    fn test_empty_string_is_not_daemon() {
        assert!(!is_in_daemon_mode(Some(""), WINDOW));
    }

    #[test]
    fn test_zero_sentinel_is_not_daemon() {
        assert!(!is_in_daemon_mode(Some("0"), WINDOW));
    }

    #[test]
    fn test_garbage_is_not_daemon() {
        assert!(!is_in_daemon_mode(Some("not-a-timestamp"), WINDOW));
        assert!(!is_in_daemon_mode(Some("9999"), WINDOW));
        assert!(!is_in_daemon_mode(Some("null"), WINDOW));
    }

    #[test]
    fn test_recent_rfc3339_within_window_is_daemon() {
        // 5 seconds ago — well within 15s window
        let ts = seconds_ago_rfc3339(5);
        assert!(
            is_in_daemon_mode(Some(&ts), WINDOW),
            "5s-ago RFC3339 should be in-window"
        );
    }

    #[test]
    fn test_recent_sqlite_within_window_is_daemon() {
        let ts = seconds_ago_sqlite(5);
        assert!(
            is_in_daemon_mode(Some(&ts), WINDOW),
            "5s-ago SQLite format should be in-window"
        );
    }

    #[test]
    fn test_just_now_rfc3339_is_daemon() {
        let ts = now_rfc3339();
        assert!(is_in_daemon_mode(Some(&ts), WINDOW));
    }

    #[test]
    fn test_just_now_sqlite_is_daemon() {
        let ts = now_sqlite();
        assert!(is_in_daemon_mode(Some(&ts), WINDOW));
    }

    #[test]
    fn test_old_rfc3339_out_of_window_is_not_daemon() {
        // 60 seconds ago — outside 15s window
        let ts = seconds_ago_rfc3339(60);
        assert!(
            !is_in_daemon_mode(Some(&ts), WINDOW),
            "60s-ago RFC3339 should be out-of-window"
        );
    }

    #[test]
    fn test_old_sqlite_out_of_window_is_not_daemon() {
        let ts = seconds_ago_sqlite(60);
        assert!(
            !is_in_daemon_mode(Some(&ts), WINDOW),
            "60s-ago SQLite format should be out-of-window"
        );
    }

    #[test]
    fn test_future_timestamp_is_not_daemon() {
        let ts = seconds_from_now_rfc3339(10);
        assert!(
            !is_in_daemon_mode(Some(&ts), WINDOW),
            "future timestamp should not be in daemon mode"
        );
    }

    #[test]
    fn test_boundary_at_exactly_window_edge() {
        // Exactly at the window boundary — should still be in-window.
        // Use a tiny window so we can set the timestamp at "exactly 0s ago"
        // and verify edge inclusion.
        let ts = now_rfc3339();
        assert!(is_in_daemon_mode(Some(&ts), Duration::from_secs(1)));
    }

    #[test]
    fn test_zero_window_just_now_is_daemon() {
        // A 0-duration window: only a timestamp that is *exactly* now would
        // pass. In practice now_rfc3339() is always <= Utc::now() so the age
        // is 0 or a few ms — which is <= Duration::ZERO only if it's exactly
        // 0. This is a boundary sanity check rather than a pass assertion.
        // We just confirm no panic and that an old timestamp fails.
        let ts = seconds_ago_rfc3339(1);
        assert!(!is_in_daemon_mode(Some(&ts), Duration::ZERO));
    }
}
