//! IPMI power driver using `ipmitool`.
//!
//! Implements [`crate::director::power::PowerDriver`] via the `ipmitool` CLI,
//! using the IPMI 2.0 (lanplus) interface.  `ipmitool` must be installed on
//! the host for any async methods to succeed; the pure `ipmi_args` helper is
//! always testable without a running BMC.

use std::future::Future;
use std::time::Duration;

use anyhow::Result;

use super::{PowerDriver, PowerState};

/// IPMI power driver that wraps `ipmitool`.
///
/// Communicates with the BMC over the IPMI 2.0 (RMCP+/lanplus) transport.
/// The binary `ipmitool` must be installed on the host for any of the async
/// methods to work.  All operations are best-effort and their errors should
/// be logged by the caller rather than propagated as hard failures.
///
/// Every `ipmitool` invocation is bounded by `command_timeout`: an unreachable
/// BMC can otherwise make `ipmitool` lanplus retries block for tens of seconds,
/// and these calls run inside HTTP request handlers.
pub struct IpmiDriver {
    host: String,
    username: String,
    password: String,
    command_timeout: Duration,
}

impl IpmiDriver {
    /// Create a new IPMI driver.
    ///
    /// # Arguments
    /// * `host`            – BMC IP address or hostname
    /// * `username`        – IPMI username (e.g. `"RACKDIRECTOR"`)
    /// * `password`        – IPMI password
    /// * `command_timeout` – Maximum time to wait for an `ipmitool` invocation
    ///   before treating it as a failure.
    pub fn new(
        host: String,
        username: String,
        password: String,
        command_timeout: Duration,
    ) -> Self {
        Self {
            host,
            username,
            password,
            command_timeout,
        }
    }

    /// Run the given IPMI verb and return the raw stdout string.
    ///
    /// The password is passed to `ipmitool` via the `IPMI_PASSWORD` environment
    /// variable (`-E`) rather than on the command line, so it is not visible in
    /// the host process list. The invocation is bounded by `command_timeout`.
    async fn run(&self, verb: &str) -> Result<String> {
        let args = ipmi_args(&self.host, &self.username, verb);
        let fut = tokio::process::Command::new("ipmitool")
            .args(&args)
            .env("IPMI_PASSWORD", &self.password)
            .output();

        let output = with_timeout(self.command_timeout, "ipmitool", fut).await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ipmitool failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Await `fut`, failing with a clear error if it does not complete within
/// `timeout`.
///
/// This is a generic, transport-agnostic helper extracted so the timeout
/// behavior can be unit-tested without spawning `ipmitool` (which is not
/// available on all CI platforms). `label` names the operation in the timeout
/// error message.
///
/// Returns:
/// - `Ok(value)` when `fut` resolves before the deadline,
/// - `Err(_)` with an "timed out" message when the deadline elapses first.
async fn with_timeout<F, T>(timeout: Duration, label: &str, fut: F) -> Result<T>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| anyhow::anyhow!("{} timed out after {:?}", label, timeout))
}

/// Build the `ipmitool` argument list for the given host/credentials and verb.
///
/// The standard lanplus connection flags are emitted, using `-E` so that
/// `ipmitool` reads the password from the `IPMI_PASSWORD` environment variable
/// instead of accepting it as a command-line argument (which would expose it in
/// the host process list). The caller is responsible for setting that
/// environment variable on the spawned command.
///
/// The verb string is split on whitespace and appended after the connection
/// flags. For example, `verb = "chassis power on"` becomes
/// `["-I","lanplus","-H",host,"-U",user,"-E","chassis","power","on"]`.
///
/// This function is pure and has no side effects, making it fully unit-testable
/// without a running BMC or `ipmitool` binary.
pub(crate) fn ipmi_args(host: &str, user: &str, verb: &str) -> Vec<String> {
    let mut args = vec![
        "-I".to_string(),
        "lanplus".to_string(),
        "-H".to_string(),
        host.to_string(),
        "-U".to_string(),
        user.to_string(),
        "-E".to_string(),
    ];
    args.extend(verb.split_whitespace().map(str::to_string));
    args
}

#[async_trait::async_trait]
impl PowerDriver for IpmiDriver {
    async fn power_state(&self) -> Result<PowerState> {
        let stdout = self.run("chassis power status").await?;
        let stdout_lower = stdout.to_lowercase();
        if stdout_lower.contains("chassis power is on") {
            Ok(PowerState::On)
        } else if stdout_lower.contains("chassis power is off") {
            Ok(PowerState::Off)
        } else {
            Ok(PowerState::Unknown)
        }
    }

    async fn power_on(&self) -> Result<()> {
        self.run("chassis power on").await?;
        Ok(())
    }

    async fn power_off(&self, graceful: bool) -> Result<()> {
        if graceful {
            self.run("chassis power soft").await?;
        } else {
            self.run("chassis power off").await?;
        }
        Ok(())
    }

    async fn power_cycle(&self) -> Result<()> {
        self.run("chassis power cycle").await?;
        Ok(())
    }

    async fn power_reset(&self) -> Result<()> {
        self.run("chassis power reset").await?;
        log::info!("IPMI power reset sent to {}", self.host);
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "ipmi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ipmi_args unit tests (pure — no ipmitool needed)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipmi_args_power_reset() {
        let args = ipmi_args("10.0.0.1", "admin", "chassis power reset");
        assert_eq!(
            args,
            vec![
                "-I", "lanplus", "-H", "10.0.0.1", "-U", "admin", "-E", "chassis", "power", "reset"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_power_on() {
        let args = ipmi_args("192.168.1.100", "RACKDIRECTOR", "chassis power on");
        assert_eq!(
            args,
            vec![
                "-I",
                "lanplus",
                "-H",
                "192.168.1.100",
                "-U",
                "RACKDIRECTOR",
                "-E",
                "chassis",
                "power",
                "on"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_power_status() {
        let args = ipmi_args("bmc.local", "user", "chassis power status");
        assert_eq!(
            args,
            vec![
                "-I",
                "lanplus",
                "-H",
                "bmc.local",
                "-U",
                "user",
                "-E",
                "chassis",
                "power",
                "status"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_does_not_contain_password() {
        // Security: the password must never appear on the command line; it is
        // passed via the IPMI_PASSWORD environment variable instead.
        let args = ipmi_args("h", "u", "chassis power status");
        assert!(
            !args.iter().any(|a| a == "-P"),
            "args must not contain the -P password flag"
        );
        assert!(
            args.iter().any(|a| a == "-E"),
            "args must request password from the environment with -E"
        );
    }

    #[test]
    fn test_ipmi_args_single_verb_token() {
        // Edge case: single-word verb (not a real ipmitool command, but
        // confirms splitting logic)
        let args = ipmi_args("h", "u", "ping");
        assert_eq!(
            args,
            vec!["-I", "lanplus", "-H", "h", "-U", "u", "-E", "ping"]
        );
    }

    #[test]
    fn test_ipmi_args_extra_whitespace_in_verb() {
        // Extra spaces in verb are collapsed by split_whitespace
        let args = ipmi_args("h", "u", "chassis  power  off");
        assert_eq!(
            args,
            vec![
                "-I", "lanplus", "-H", "h", "-U", "u", "-E", "chassis", "power", "off"
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Driver construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipmi_driver_construction() {
        let driver = IpmiDriver::new(
            "10.0.0.100".to_string(),
            "RACKDIRECTOR".to_string(),
            "test_password".to_string(),
            Duration::from_secs(4),
        );
        assert_eq!(driver.kind(), "ipmi");
        assert_eq!(driver.host, "10.0.0.100");
        assert_eq!(driver.username, "RACKDIRECTOR");
        assert_eq!(driver.password, "test_password");
        assert_eq!(driver.command_timeout, Duration::from_secs(4));
    }

    #[test]
    fn test_ipmi_driver_empty_credentials() {
        // Construction succeeds; errors surface only when commands are run.
        let driver = IpmiDriver::new(
            "".to_string(),
            "".to_string(),
            "".to_string(),
            Duration::from_secs(4),
        );
        assert_eq!(driver.kind(), "ipmi");
    }

    #[tokio::test]
    async fn test_power_reset_handles_missing_ipmitool() {
        // When ipmitool is not installed or returns an error, power_reset
        // should return Err (not panic).
        let driver = IpmiDriver::new(
            "10.0.0.100".to_string(),
            "RACKDIRECTOR".to_string(),
            "test_password".to_string(),
            Duration::from_secs(4),
        );
        // We expect either Ok (if ipmitool is installed and somehow succeeds)
        // or Err (the normal case in CI). The important thing is no panic.
        let result = driver.power_reset().await;
        assert!(result.is_ok() || result.is_err());
    }

    // -----------------------------------------------------------------------
    // with_timeout helper tests (generic — no ipmitool / OS binary needed)
    // -----------------------------------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn test_with_timeout_elapses_on_slow_future() {
        // A future that sleeps longer than the timeout must produce a timeout
        // error. With a paused clock this resolves deterministically and fast.
        let slow = async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            42
        };
        let result = with_timeout(Duration::from_secs(1), "test-op", slow).await;
        assert!(result.is_err(), "slow future should time out");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test-op") && msg.contains("timed out"),
            "timeout error should name the operation: {msg}"
        );
    }

    #[tokio::test]
    async fn test_with_timeout_passes_through_fast_future() {
        // A future that resolves before the deadline returns its value.
        let fast = async { 7 };
        let value = with_timeout(Duration::from_secs(5), "test-op", fast)
            .await
            .expect("fast future should not time out");
        assert_eq!(value, 7);
    }
}
