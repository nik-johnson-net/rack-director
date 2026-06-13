//! Power-kick helpers and the user-facing `PowerAction` enum.
//!
//! These free functions are the operational layer that sits between a resolved
//! [`PowerDriver`] and the `Director` methods.  They handle the state-machine
//! logic for choosing the right power command (on / cycle / cycle-with-fallback)
//! and absorb errors so that callers always receive `Ok(())`.

use super::{PowerDriver, PowerState};
use crate::plans::actions::Action;

/// User-requested power action for the UI power controls.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerAction {
    /// Power the device on.
    On,
    /// Power the device off (hard/immediate power-off, not a graceful OS shutdown).
    ///
    /// A hard off is used because hosts are frequently running the rack-agent in
    /// an initramfs that does not handle ACPI soft-off, so a graceful shutdown can
    /// silently hang. This also matches the UI confirm dialog's promise that the
    /// device will lose power immediately.
    Off,
    /// Power-cycle the device (off then on).
    Cycle,
}

/// Returns `true` if `action` requires the device to be booted into the rack-agent
/// before it can be executed.
///
/// Actions that need the agent running on the device return `true` and will trigger an
/// OOB power kick when the device is not already in daemon mode. `RebootDevice` returns
/// `false` because its `start()` hook already handles the reboot via `Director::reboot`.
pub(crate) fn action_requires_boot(action: &Action) -> bool {
    matches!(
        action,
        Action::DiscoverHardware
            | Action::ConfigureBmc
            | Action::PartitionDisks
            | Action::InstallOs
            | Action::Console
    )
}

/// Apply a power kick using the already-resolved driver.
///
/// Queries the current power state and issues the appropriate command:
/// - `Off`        → `power_on`
/// - `On`         → `power_cycle` (reprovision path — device likely running an OS)
/// - `Unknown`    → `power_cycle`, with a `power_on` fallback if the cycle fails
/// - state query error → treated as `Unknown` (log warn + `power_cycle` with fallback)
///
/// The `Unknown` fallback exists because Redfish `ForceRestart` (and IPMI
/// `chassis power cycle`) typically error when the host is actually OFF. In that
/// case a plain `power_on` is the correct recovery, so we attempt it rather than
/// giving up after a single failed cycle. The confirmed-`On` case does NOT fall
/// back to `power_on`: a device that is genuinely on should not be powered on
/// again if its cycle command fails.
///
/// Every power operation is best-effort: an error is logged but never propagated.
/// Returns `Ok(())` in all cases.
pub(crate) async fn apply_power_kick(driver: &dyn PowerDriver) -> anyhow::Result<()> {
    let state = match driver.power_state().await {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "Could not query power state via {} driver: {} — treating as Unknown",
                driver.kind(),
                e
            );
            PowerState::Unknown
        }
    };

    match state {
        PowerState::Off => {
            log::info!(
                "Device is off — issuing power_on via {} driver",
                driver.kind()
            );
            log_if_err(driver.power_on().await, driver.kind());
        }
        PowerState::On => {
            log::info!(
                "Device is On — issuing power_cycle via {} driver",
                driver.kind()
            );
            log_if_err(driver.power_cycle().await, driver.kind());
        }
        PowerState::Unknown => cycle_with_power_on_fallback(driver).await,
    }

    Ok(())
}

/// Issue a `power_cycle` for a device in an `Unknown` state, falling back to
/// `power_on` if the cycle fails.
///
/// A failed cycle on an Unknown host most often means the host is actually OFF
/// (Redfish `ForceRestart` errors in that state), so `power_on` is the correct
/// best-effort recovery. Both operations log on failure but never propagate.
async fn cycle_with_power_on_fallback(driver: &dyn PowerDriver) {
    log::info!(
        "Device is Unknown — issuing power_cycle via {} driver",
        driver.kind()
    );
    if let Err(e) = driver.power_cycle().await {
        log::warn!(
            "power_cycle failed via {} driver: {} — falling back to power_on",
            driver.kind(),
            e
        );
        log_if_err(driver.power_on().await, driver.kind());
    }
}

/// Log a best-effort power command failure without propagating it.
fn log_if_err(result: anyhow::Result<()>, kind: &str) {
    if let Err(e) = result {
        log::warn!("Power kick command failed via {} driver: {}", kind, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // FakeDriver — records every command issued and returns a configurable state
    // -----------------------------------------------------------------------

    /// A fake `PowerDriver` that records every command issued (in order) and
    /// returns a configurable state from `power_state`.
    struct FakeDriver {
        state: PowerState,
        /// Tracks every power command issued, in order: "on", "off", "cycle", "reset".
        commands: Mutex<Vec<String>>,
        /// If true, `power_state()` returns an error instead.
        state_error: bool,
        /// If true, `power_cycle()` returns an error instead of succeeding.
        cycle_fails: bool,
    }

    impl FakeDriver {
        fn new(state: PowerState) -> Self {
            Self {
                state,
                commands: Mutex::new(Vec::new()),
                state_error: false,
                cycle_fails: false,
            }
        }

        fn with_state_error() -> Self {
            Self {
                state: PowerState::Unknown,
                commands: Mutex::new(Vec::new()),
                state_error: true,
                cycle_fails: false,
            }
        }

        /// Build a driver in `state` whose `power_cycle()` always fails.
        fn with_cycle_failure(state: PowerState) -> Self {
            Self {
                state,
                commands: Mutex::new(Vec::new()),
                state_error: false,
                cycle_fails: true,
            }
        }

        fn record(&self, command: &str) {
            self.commands.lock().unwrap().push(command.to_string());
        }

        /// All commands issued so far, in order.
        fn commands(&self) -> Vec<String> {
            self.commands.lock().unwrap().clone()
        }

        /// The most recent command issued, if any.
        fn last_command(&self) -> Option<String> {
            self.commands.lock().unwrap().last().cloned()
        }
    }

    #[async_trait::async_trait]
    impl PowerDriver for FakeDriver {
        async fn power_state(&self) -> anyhow::Result<PowerState> {
            if self.state_error {
                Err(anyhow::anyhow!("simulated power_state error"))
            } else {
                Ok(self.state)
            }
        }

        async fn power_on(&self) -> anyhow::Result<()> {
            self.record("on");
            Ok(())
        }

        async fn power_off(&self, _graceful: bool) -> anyhow::Result<()> {
            self.record("off");
            Ok(())
        }

        async fn power_cycle(&self) -> anyhow::Result<()> {
            self.record("cycle");
            if self.cycle_fails {
                Err(anyhow::anyhow!("simulated power_cycle failure"))
            } else {
                Ok(())
            }
        }

        async fn power_reset(&self) -> anyhow::Result<()> {
            self.record("reset");
            Ok(())
        }

        fn kind(&self) -> &'static str {
            "fake"
        }
    }

    // -----------------------------------------------------------------------
    // action_requires_boot table tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_action_requires_boot_discover_hardware() {
        assert!(action_requires_boot(&Action::DiscoverHardware));
    }

    #[test]
    fn test_action_requires_boot_configure_bmc() {
        assert!(action_requires_boot(&Action::ConfigureBmc));
    }

    #[test]
    fn test_action_requires_boot_partition_disks() {
        assert!(action_requires_boot(&Action::PartitionDisks));
    }

    #[test]
    fn test_action_requires_boot_install_os() {
        assert!(action_requires_boot(&Action::InstallOs));
    }

    #[test]
    fn test_action_requires_boot_console() {
        assert!(action_requires_boot(&Action::Console));
    }

    #[test]
    fn test_action_requires_boot_reboot_device_is_false() {
        assert!(!action_requires_boot(&Action::RebootDevice));
    }

    // -----------------------------------------------------------------------
    // apply_power_kick tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_apply_power_kick_when_off_calls_power_on() {
        let driver = FakeDriver::new(PowerState::Off);
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(driver.last_command(), Some("on".to_string()));
    }

    #[tokio::test]
    async fn test_apply_power_kick_when_on_calls_power_cycle() {
        let driver = FakeDriver::new(PowerState::On);
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(driver.last_command(), Some("cycle".to_string()));
    }

    #[tokio::test]
    async fn test_apply_power_kick_when_unknown_calls_power_cycle() {
        let driver = FakeDriver::new(PowerState::Unknown);
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(driver.last_command(), Some("cycle".to_string()));
    }

    #[tokio::test]
    async fn test_apply_power_kick_when_state_error_calls_power_cycle() {
        let driver = FakeDriver::with_state_error();
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(driver.last_command(), Some("cycle".to_string()));
    }

    #[tokio::test]
    async fn test_apply_power_kick_unknown_cycle_fails_falls_back_to_power_on() {
        // An Unknown host whose power_cycle fails is most likely actually OFF
        // (Redfish ForceRestart errors when off) — we should fall back to power_on.
        let driver = FakeDriver::with_cycle_failure(PowerState::Unknown);
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(
            driver.commands(),
            vec!["cycle".to_string(), "on".to_string()],
            "Unknown + cycle failure should attempt power_cycle then fall back to power_on"
        );
    }

    #[tokio::test]
    async fn test_apply_power_kick_on_cycle_fails_does_not_fall_back() {
        // A confirmed-On host whose power_cycle fails must NOT be powered on again.
        let driver = FakeDriver::with_cycle_failure(PowerState::On);
        apply_power_kick(&driver).await.unwrap();
        assert_eq!(
            driver.commands(),
            vec!["cycle".to_string()],
            "On + cycle failure should NOT fall back to power_on"
        );
    }

    #[tokio::test]
    async fn test_apply_power_kick_returns_ok_even_if_power_op_fails() {
        // A driver where every power command fails
        struct FailDriver;
        #[async_trait::async_trait]
        impl PowerDriver for FailDriver {
            async fn power_state(&self) -> anyhow::Result<PowerState> {
                Ok(PowerState::On)
            }
            async fn power_on(&self) -> anyhow::Result<()> {
                Err(anyhow::anyhow!("fail"))
            }
            async fn power_off(&self, _: bool) -> anyhow::Result<()> {
                Err(anyhow::anyhow!("fail"))
            }
            async fn power_cycle(&self) -> anyhow::Result<()> {
                Err(anyhow::anyhow!("fail"))
            }
            async fn power_reset(&self) -> anyhow::Result<()> {
                Err(anyhow::anyhow!("fail"))
            }
            fn kind(&self) -> &'static str {
                "fail"
            }
        }

        // Even when the power command fails, apply_power_kick returns Ok(())
        let result = apply_power_kick(&FailDriver).await;
        assert!(result.is_ok());
    }
}
