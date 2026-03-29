use anyhow::Result;
use log::{error, info, warn};

use common::cnc::{CncClient, PollAction, PollResponse};

use crate::{bmc, partition, scan};

const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Outcome of a single poll loop iteration that determines whether to
/// sleep, poll immediately again, or shut down the daemon.
enum LoopControl {
    /// Skip the sleep delay and poll again immediately (e.g. after completing an action).
    PollImmediately,
    /// Sleep for [`POLL_INTERVAL`] before the next poll (e.g. no work available).
    SleepThenPoll,
    /// Exit the daemon cleanly (e.g. after an action that requires a reboot).
    Exit,
}

/// Run the agent in daemon mode, polling rack-director for actions.
///
/// Resolves the device UUID from SMBIOS once at startup. Polls `GET /cnc/poll`
/// every [`POLL_INTERVAL`] seconds when idle. Dispatches received actions to the
/// appropriate handler. Exits cleanly on `RebootDevice` or `InstallOs` so that
/// the caller (e.g. systemd) can handle the reboot.
pub async fn run_daemon(client: &CncClient) -> Result<()> {
    let uuid = scan::read_dmi_for_uuid()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Could not read device UUID from SMBIOS/DMI tables"))?;

    info!("Daemon mode started, device UUID: {}", uuid);

    loop {
        let control = match client.poll(&uuid).await {
            Err(e) => {
                warn!("Poll failed: {e}");
                LoopControl::SleepThenPoll
            }
            Ok(None) => LoopControl::SleepThenPoll,
            Ok(Some(PollResponse::Action { payload })) => {
                info!("Received action: {:?}", payload);
                dispatch_action(client, &payload).await
            }
        };

        match control {
            LoopControl::PollImmediately => continue,
            LoopControl::SleepThenPoll => {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            LoopControl::Exit => return Ok(()),
        }
    }
}

/// Dispatch a received action to the appropriate handler and return the
/// [`LoopControl`] value that should govern the next loop iteration.
///
/// Action handlers are responsible for reporting success or failure back to
/// rack-director via [`RackDirector::action_success`] /
/// [`RackDirector::action_failed`]. The daemon only observes the outcome to
/// decide whether to poll again immediately or stop.
async fn dispatch_action(client: &CncClient, action: &PollAction) -> LoopControl {
    match action {
        PollAction::DiscoverHardware => {
            let args = scan::DeviceScanArgs::new(false);
            match scan::device_scan(client, &args).await {
                Ok(_) => LoopControl::PollImmediately,
                Err(e) => {
                    error!("DiscoverHardware failed: {e}");
                    LoopControl::SleepThenPoll
                }
            }
        }
        PollAction::ConfigureBmc => match bmc::bmc_configure(client).await {
            Ok(_) => LoopControl::PollImmediately,
            Err(e) => {
                error!("ConfigureBmc failed: {e}");
                LoopControl::SleepThenPoll
            }
        },
        PollAction::PartitionDisks => match partition::partition_disks(client).await {
            Ok(_) => LoopControl::PollImmediately,
            Err(e) => {
                error!("PartitionDisks failed: {e}");
                LoopControl::SleepThenPoll
            }
        },
        PollAction::RebootDevice | PollAction::InstallOs => {
            info!("Action {:?} requires reboot — exiting daemon", action);
            // EXIT WITHOUT calling action_success. This is intentional — do not
            // "fix" this by adding an action_success call here.
            //
            // Plan advancement for these actions is handled externally:
            //
            // RebootDevice: rack-director's on_boot() fires on the next PXE boot.
            //   Because advance_on_boot() == true for RebootDevice, on_boot()
            //   automatically advances the plan. Calling action_success here would
            //   cause a double-advance: once from action_success, then again from
            //   on_boot().
            //
            // InstallOs: rack-director's on_boot() fires on the next PXE boot.
            //   Because advance_on_boot() == false for InstallOs, on_boot() does NOT
            //   advance the plan — instead it serves the OS installer boot target. The
            //   OS installer itself calls action_success when the installation is done.
            //   Calling action_success here would advance the plan before the OS is
            //   installed, causing the installer to never run.
            //
            // Known limitation: if the machine crashes after the daemon exits but
            // before rebooting, the plan remains in 'running' state indefinitely with
            // no timeout. This requires manual intervention to reset the plan.
            LoopControl::Exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that `dispatch_action` returns `Exit` for `RebootDevice`.
    ///
    /// We cannot easily spin up a full RackDirector client for unit tests, so
    /// we test `dispatch_action` by providing a mock server that the client
    /// connects to and never actually calls handlers for exit-path actions.
    #[tokio::test]
    async fn test_dispatch_reboot_device_exits() {
        let mut server = mockito::Server::new_async().await;
        // No mock needed — RebootDevice exits without making network calls.
        let _mock = server
            .mock("POST", "/cnc/action_success")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let control = dispatch_action(&client, &PollAction::RebootDevice).await;

        assert!(matches!(control, LoopControl::Exit));
    }

    /// Test that `dispatch_action` returns `Exit` for `InstallOs`.
    #[tokio::test]
    async fn test_dispatch_install_os_exits() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cnc/action_success")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let control = dispatch_action(&client, &PollAction::InstallOs).await;

        assert!(matches!(control, LoopControl::Exit));
    }

    /// Test that `dispatch_action` returns `SleepThenPoll` when `DiscoverHardware`
    /// fails (no SMBIOS tables available in the test environment).
    ///
    /// This prevents a tight busy-loop when the handler fails before calling
    /// `action_failed` — sleeping allows time for transient issues to resolve.
    #[tokio::test]
    async fn test_dispatch_discover_hardware_sleeps_on_error() {
        let mut server = mockito::Server::new_async().await;
        // The scan will fail (no SMBIOS in test env); dispatch_action must return
        // SleepThenPoll to avoid a tight error loop.
        let _mock = server
            .mock("POST", "/cnc/action_failed")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let control = dispatch_action(&client, &PollAction::DiscoverHardware).await;

        assert!(matches!(control, LoopControl::SleepThenPoll));
    }

    /// Test that `dispatch_action` returns `SleepThenPoll` when `PartitionDisks`
    /// fails (no disk layout available in the test environment).
    #[tokio::test]
    async fn test_dispatch_partition_disks_sleeps_on_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cnc/action_failed")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let control = dispatch_action(&client, &PollAction::PartitionDisks).await;

        assert!(matches!(control, LoopControl::SleepThenPoll));
    }

    /// Test that `dispatch_action` returns `SleepThenPoll` when `ConfigureBmc`
    /// fails (no BMC or IPMI tools available in the test environment).
    #[tokio::test]
    async fn test_dispatch_configure_bmc_sleeps_on_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cnc/action_failed")
            .create_async()
            .await;

        let client = CncClient::new(&server.url());
        let control = dispatch_action(&client, &PollAction::ConfigureBmc).await;

        assert!(matches!(control, LoopControl::SleepThenPoll));
    }
}
