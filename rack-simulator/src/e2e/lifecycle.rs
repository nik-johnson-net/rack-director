use anyhow::{Result, anyhow};
use std::time::Duration;
use tokio::time::sleep;

use crate::e2e::config::LifecycleStep;
use crate::e2e::director::DirectorVm;

/// Drive a device through a sequence of lifecycle steps.
///
/// For each step, verifies the current state, starts the transition, and waits
/// for the device to reach the target state.
pub async fn drive_lifecycle(
    director: &DirectorVm,
    device_uuid: &str,
    steps: &[LifecycleStep],
    timeout: Duration,
) -> Result<()> {
    for step in steps {
        let current = director.get_lifecycle_state(device_uuid).await?;
        if current != step.from {
            return Err(anyhow!(
                "Expected device to be in state '{}' but found '{}'",
                step.from,
                current
            ));
        }

        director.start_transition(device_uuid, &step.to).await?;
        wait_for_state(director, device_uuid, &step.to, timeout).await?;
    }
    Ok(())
}

/// Poll device state until it matches `target` or an error state is reached.
async fn wait_for_state(
    director: &DirectorVm,
    uuid: &str,
    target: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Device did not reach state '{}' within {:?}",
                target,
                timeout
            ));
        }

        let state = director.get_lifecycle_state(uuid).await?;

        if state == target {
            return Ok(());
        }

        if state == "broken" || state == "failed" {
            return Err(anyhow!(
                "Device entered error state '{}' while waiting for '{}'",
                state,
                target
            ));
        }

        sleep(Duration::from_secs(2)).await;
    }
}
