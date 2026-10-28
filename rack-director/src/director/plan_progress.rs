//! Plan-progress methods on `Director`: advancing plans on boot and agent reports,
//! cancelling transitions, and repairing orphaned transitions.
//!
//! A plan and its lifecycle transition must change together. The UI treats a device as
//! mid-transition while its `lifecycle_transitions` row is open, but iPXE only serves work
//! for an active (pending/running) plan. If a plan finishes while its transition stays
//! open, the device sleeps forever and can be neither cancelled nor re-transitioned.
//! Every write here therefore runs in a single `BEGIN IMMEDIATE` transaction.

use uuid::Uuid;

use crate::database::Connection;
use crate::lifecycle::store as lifecycle_store;
use crate::lifecycle::{DeviceLifecycle, LifecycleTransition};
use crate::plans::store as plan_store;
use crate::plans::{Plan, PlanStatus};

impl<'a> super::Director<'a> {
    /// Handle device boot event
    ///
    /// Called when a device boots via iPXE. If the current action's advance_on_boot()
    /// returns true, the action is automatically marked as successful and the plan advances.
    /// This is used for actions like RebootDevice that complete when the device boots.
    ///
    /// With no active plan, an open transition is repaired here (see
    /// [`Self::repair_orphaned_transition`]) before iPXE picks the boot target, so a stuck
    /// device recovers on its next boot.
    pub async fn on_boot(&mut self, device_uuid: &Uuid) -> anyhow::Result<()> {
        let found = self
            .update_active_plan(device_uuid, |plan| {
                // Start the plan if it's pending
                let plan_was_pending = plan.status == PlanStatus::Pending;
                if plan_was_pending {
                    plan.start();
                }

                let should_advance = match plan.get_current_action() {
                    Some(action) if action.advance_on_boot() => {
                        log::info!(
                            "Device {} booted - advancing action {:?}",
                            device_uuid,
                            action
                        );
                        true
                    }
                    Some(action) => {
                        log::debug!(
                            "Device {} booted but current action {:?} does not advance on boot",
                            device_uuid,
                            action
                        );
                        false
                    }
                    None => false,
                };
                if should_advance {
                    plan.mark_action_success();
                }

                plan_was_pending || should_advance
            })
            .await?;

        if !found && !self.repair_orphaned_transition(device_uuid).await? {
            // No active plan - this is normal for devices that have completed their lifecycle
            log::debug!("No active plan for device {} on boot", device_uuid);
        }
        Ok(())
    }

    /// Mark the current action for a device as successful, advancing the plan.
    ///
    /// If `reported_plan_id` is `Some`, it is verified against the active plan's
    /// ID. A mismatch means the report is stale (sent by an agent that was
    /// running a since-cancelled plan), and the call is silently ignored so that
    /// the new plan is not corrupted. When `reported_plan_id` is `None` (old
    /// agent versions), the report is applied unconditionally.
    pub async fn mark_action_success(
        &mut self,
        device_uuid: &Uuid,
        reported_plan_id: Option<i64>,
    ) -> anyhow::Result<()> {
        let found = self
            .update_active_plan(device_uuid, |plan| {
                if is_stale_report(device_uuid, plan, reported_plan_id, "action_success") {
                    return false;
                }
                if plan.status == PlanStatus::Pending {
                    plan.start();
                }
                plan.mark_action_success();
                true
            })
            .await?;

        if !found {
            // No active plan: the plan was likely cancelled while the agent was in-flight.
            // This is expected and not an error from the agent's perspective.
            log::debug!(
                "action_success for {} but no active plan found — ignoring (plan may have been cancelled)",
                device_uuid
            );
        }
        Ok(())
    }

    /// Mark the current action for a device as failed, failing the plan.
    ///
    /// `reported_plan_id` is checked the same way as in [`Self::mark_action_success`].
    pub async fn mark_action_failed(
        &mut self,
        device_uuid: &Uuid,
        error_message: &str,
        reported_plan_id: Option<i64>,
    ) -> anyhow::Result<()> {
        let found = self
            .update_active_plan(device_uuid, |plan| {
                if is_stale_report(device_uuid, plan, reported_plan_id, "action_failed") {
                    return false;
                }
                plan.mark_action_failed(error_message.to_string());
                true
            })
            .await?;

        if !found {
            // No active plan: the plan was likely cancelled while the agent was in-flight.
            log::debug!(
                "action_failed for {} but no active plan found — ignoring (plan may have been cancelled)",
                device_uuid
            );
        }
        Ok(())
    }

    /// Cancel the active lifecycle transition for a device, moving it to Broken.
    ///
    /// Also closes an orphaned transition whose plan already finished, so operators can
    /// always recover a device stuck showing an active transition. That plan's outcome is
    /// applied rather than overridden, so a stranded successful plan isn't thrown away.
    pub async fn cancel_active_transition(&mut self, device_uuid: &Uuid) -> anyhow::Result<()> {
        self.conn
            .in_transaction(async |conn| cancel_transition(conn, device_uuid).await)
            .await
    }

    /// Atomically load the device's active plan, let `apply` mutate it, persist it, and
    /// close the lifecycle transition if the plan finished. `apply` returns whether it
    /// changed the plan. Returns `false` if the device has no active plan.
    ///
    /// `apply` first runs on an unlocked read, because most boots and reports change nothing
    /// and `BEGIN IMMEDIATE` takes the database-wide write lock. It runs again inside the
    /// transaction only if the plan moved between the read and taking the lock.
    async fn update_active_plan<F>(
        &mut self,
        device_uuid: &Uuid,
        mut apply: F,
    ) -> anyhow::Result<bool>
    where
        F: FnMut(&mut Plan) -> bool,
    {
        let Some(snapshot) = plan_store::get_active_plan_for_device(self.conn, device_uuid).await?
        else {
            return Ok(false);
        };
        let mut plan = snapshot.clone();
        if !apply(&mut plan) {
            return Ok(true);
        }

        self.conn
            .in_transaction(async |conn| {
                let Some(mut current) =
                    plan_store::get_active_plan_for_device(conn, device_uuid).await?
                else {
                    return Ok(false);
                };
                if !same_progress(&current, &snapshot) {
                    if !apply(&mut current) {
                        return Ok(true);
                    }
                    plan = current;
                }
                save_plan(conn, &plan).await?;
                Ok(true)
            })
            .await
    }

    /// Close an open transition whose plan has already finished, applying the plan's
    /// outcome (missing plan → Broken). Returns whether a transition was repaired.
    ///
    /// Plans and transitions are always written together, so this should never fire; it
    /// guards against a future bug stranding a device, and logs a warning when it does.
    async fn repair_orphaned_transition(&mut self, device_uuid: &Uuid) -> anyhow::Result<bool> {
        // Cheap unlocked check first: almost every boot has no open transition.
        if lifecycle_store::get_active_transition_for_device(self.conn, device_uuid)
            .await?
            .is_none()
        {
            return Ok(false);
        }

        self.conn
            .in_transaction(async |conn| {
                let Some(transition) =
                    lifecycle_store::get_active_transition_for_device(conn, device_uuid).await?
                else {
                    return Ok(false);
                };
                let plan = get_transition_plan(conn, &transition).await?;
                let outcome = match &plan {
                    Some(plan) => plan_outcome(plan),
                    None => Some((false, Some("Plan missing"))),
                };
                let Some((success, error_message)) = outcome else {
                    // The plan is still active, so the transition isn't orphaned.
                    return Ok(false);
                };

                log::warn!(
                    "Repairing orphaned transition {:?} for device {}: plan {:?} is {:?} but the transition was never closed",
                    transition.id,
                    device_uuid,
                    transition.plan_id,
                    plan.as_ref().map(|p| &p.status)
                );
                close_transition(conn, &transition, success, error_message).await?;
                Ok(true)
            })
            .await
    }
}

/// Cancel the device's open transition and its own plan. If the plan already finished,
/// the transition is closed with that plan's outcome instead.
async fn cancel_transition(conn: &Connection, device_uuid: &Uuid) -> anyhow::Result<()> {
    let transition = lifecycle_store::get_active_transition_for_device(conn, device_uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No active transition to cancel"))?;

    let cancelled = match transition.plan_id {
        Some(plan_id) => plan_store::cancel_plan(conn, plan_id).await?,
        None => false,
    };
    let finished_plan = if cancelled {
        None
    } else {
        get_transition_plan(conn, &transition).await?
    };
    let (success, error_message) = finished_plan
        .as_ref()
        .and_then(plan_outcome)
        .unwrap_or((false, Some("Cancelled by user")));
    if !cancelled {
        log::warn!(
            "Cancelling orphaned transition {:?} for device {}: plan {:?} is {:?}, closing with success={}",
            transition.id,
            device_uuid,
            transition.plan_id,
            finished_plan.as_ref().map(|p| &p.status),
            success
        );
    }

    close_transition(conn, &transition, success, error_message).await
}

/// True if `a` and `b` are the same plan at the same point, so an `apply` result computed
/// from one is valid for the other.
fn same_progress(a: &Plan, b: &Plan) -> bool {
    a.id == b.id && a.status == b.status && a.current_step == b.current_step
}

/// Persist `plan`'s progress, closing its lifecycle transition if the plan finished.
async fn save_plan(conn: &Connection, plan: &Plan) -> anyhow::Result<()> {
    let plan_id = plan
        .id
        .ok_or_else(|| anyhow::anyhow!("Plan record has no ID"))?;
    plan_store::update_plan_status(
        conn,
        plan_id,
        plan.status.clone(),
        plan.current_step,
        plan.error_message.as_deref(),
    )
    .await?;
    if let Some((success, error_message)) = plan_outcome(plan)
        && let Some(transition) = lifecycle_store::get_transition_by_plan_id(conn, plan_id).await?
    {
        close_transition(conn, &transition, success, error_message).await?;
    }
    Ok(())
}

/// Load the plan a transition points at, if it has one and it still exists.
async fn get_transition_plan(
    conn: &Connection,
    transition: &LifecycleTransition,
) -> anyhow::Result<Option<Plan>> {
    match transition.plan_id {
        Some(plan_id) => plan_store::get_plan(conn, plan_id).await,
        None => Ok(None),
    }
}

/// True if the agent reported a plan ID that doesn't match the active plan.
fn is_stale_report(uuid: &Uuid, plan: &Plan, reported_plan_id: Option<i64>, kind: &str) -> bool {
    match reported_plan_id {
        Some(rid) if plan.id != Some(rid) => {
            log::warn!(
                "{} for device {} reported plan_id={} but active plan is {:?} — ignoring stale report",
                kind,
                uuid,
                rid,
                plan.id
            );
            true
        }
        _ => false,
    }
}

/// The transition outcome for a finished plan as `(success, error_message)`, or `None`
/// while the plan is still pending/running.
fn plan_outcome(plan: &Plan) -> Option<(bool, Option<&str>)> {
    match plan.status {
        PlanStatus::Success => Some((true, None)),
        PlanStatus::Failed | PlanStatus::Cancelled => Some((false, plan.error_message.as_deref())),
        PlanStatus::Pending | PlanStatus::Running => None,
    }
}

/// Close `transition` and move the device to its target state on success, or Broken on
/// failure. Skips the lifecycle update if the transition was already closed.
async fn close_transition(
    conn: &Connection,
    transition: &LifecycleTransition,
    success: bool,
    error_message: Option<&str>,
) -> anyhow::Result<()> {
    let transition_id = transition
        .id
        .ok_or_else(|| anyhow::anyhow!("Transition record has no ID"))?;
    if lifecycle_store::complete_transition(conn, transition_id, success, error_message).await? == 0
    {
        log::warn!(
            "Transition {} for device {} was already closed; skipping lifecycle update",
            transition_id,
            transition.device_uuid
        );
        return Ok(());
    }

    let lifecycle = if success {
        transition.to_state.clone()
    } else {
        DeviceLifecycle::Broken
    };
    lifecycle_store::update_device_lifecycle(conn, &transition.device_uuid, lifecycle).await
}
