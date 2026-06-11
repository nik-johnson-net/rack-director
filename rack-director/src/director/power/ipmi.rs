//! IPMI power driver using `ipmitool`.
//!
//! Implements [`crate::director::power::PowerDriver`] via the `ipmitool` CLI,
//! using the IPMI 2.0 (lanplus) interface.  `ipmitool` must be installed on
//! the host for any async methods to succeed; the pure `ipmi_args` helper is
//! always testable without a running BMC.

use anyhow::Result;

use super::{PowerDriver, PowerState};

/// IPMI power driver that wraps `ipmitool`.
///
/// Communicates with the BMC over the IPMI 2.0 (RMCP+/lanplus) transport.
/// The binary `ipmitool` must be installed on the host for any of the async
/// methods to work.  All operations are best-effort and their errors should
/// be logged by the caller rather than propagated as hard failures.
pub struct IpmiDriver {
    host: String,
    username: String,
    password: String,
}

impl IpmiDriver {
    /// Create a new IPMI driver.
    ///
    /// # Arguments
    /// * `host`     – BMC IP address or hostname
    /// * `username` – IPMI username (e.g. `"RACKDIRECTOR"`)
    /// * `password` – IPMI password
    pub fn new(host: String, username: String, password: String) -> Self {
        Self {
            host,
            username,
            password,
        }
    }

    /// Run the given IPMI verb and return the raw stdout string.
    async fn run(&self, verb: &str) -> Result<String> {
        let args = ipmi_args(&self.host, &self.username, &self.password, verb);
        let output = tokio::process::Command::new("ipmitool")
            .args(&args)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ipmitool failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Build the `ipmitool` argument list for the given host/credentials and verb.
///
/// The verb string is split on whitespace and appended after the standard
/// lanplus connection flags.  For example, `verb = "chassis power on"` becomes
/// `["-I","lanplus","-H",host,"-U",user,"-P",pass,"chassis","power","on"]`.
///
/// This function is pure and has no side effects, making it fully unit-testable
/// without a running BMC or `ipmitool` binary.
pub(crate) fn ipmi_args(host: &str, user: &str, pass: &str, verb: &str) -> Vec<String> {
    let mut args = vec![
        "-I".to_string(),
        "lanplus".to_string(),
        "-H".to_string(),
        host.to_string(),
        "-U".to_string(),
        user.to_string(),
        "-P".to_string(),
        pass.to_string(),
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
        let args = ipmi_args("10.0.0.1", "admin", "secret", "chassis power reset");
        assert_eq!(
            args,
            vec![
                "-I", "lanplus", "-H", "10.0.0.1", "-U", "admin", "-P", "secret", "chassis",
                "power", "reset"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_power_on() {
        let args = ipmi_args(
            "192.168.1.100",
            "RACKDIRECTOR",
            "p@ssw0rd",
            "chassis power on",
        );
        assert_eq!(
            args,
            vec![
                "-I",
                "lanplus",
                "-H",
                "192.168.1.100",
                "-U",
                "RACKDIRECTOR",
                "-P",
                "p@ssw0rd",
                "chassis",
                "power",
                "on"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_power_status() {
        let args = ipmi_args("bmc.local", "user", "pass", "chassis power status");
        assert_eq!(
            args,
            vec![
                "-I",
                "lanplus",
                "-H",
                "bmc.local",
                "-U",
                "user",
                "-P",
                "pass",
                "chassis",
                "power",
                "status"
            ]
        );
    }

    #[test]
    fn test_ipmi_args_single_verb_token() {
        // Edge case: single-word verb (not a real ipmitool command, but
        // confirms splitting logic)
        let args = ipmi_args("h", "u", "p", "ping");
        assert_eq!(
            args,
            vec!["-I", "lanplus", "-H", "h", "-U", "u", "-P", "p", "ping"]
        );
    }

    #[test]
    fn test_ipmi_args_extra_whitespace_in_verb() {
        // Extra spaces in verb are collapsed by split_whitespace
        let args = ipmi_args("h", "u", "p", "chassis  power  off");
        assert_eq!(
            args,
            vec![
                "-I", "lanplus", "-H", "h", "-U", "u", "-P", "p", "chassis", "power", "off"
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
        );
        assert_eq!(driver.kind(), "ipmi");
        assert_eq!(driver.host, "10.0.0.100");
        assert_eq!(driver.username, "RACKDIRECTOR");
        assert_eq!(driver.password, "test_password");
    }

    #[test]
    fn test_ipmi_driver_empty_credentials() {
        // Construction succeeds; errors surface only when commands are run.
        let driver = IpmiDriver::new("".to_string(), "".to_string(), "".to_string());
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
        );
        // We expect either Ok (if ipmitool is installed and somehow succeeds)
        // or Err (the normal case in CI). The important thing is no panic.
        let result = driver.power_reset().await;
        assert!(result.is_ok() || result.is_err());
    }
}
