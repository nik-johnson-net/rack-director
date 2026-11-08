//! Deferred device requests: work a registered device should do the next time it is idle.
//!
//! Two kinds of request turn into a plan when the device boots or polls with no active plan:
//! - **rediscovery**: an operator asked for a hardware rescan (`rediscover_requested`).
//! - **pending BMC config**: the desired `bmc_config` differs from what the last scan detected
//!   on the BMC. The plan reconfigures the BMC, then rescans so the detected state catches up.
//!
//! A request is consumed when its plan is created, not when the plan finishes: a plan that
//! fails or is cancelled must not be retried on every poll. `last_request_plan_id` points at
//! that plan so operators can see how it went. A BMC config is tried automatically once per
//! distinct network setting (`bmc_config_attempted`), so a setting the BMC won't take doesn't
//! divert every boot.

mod bmc;
mod store;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::Director;
pub use bmc::BmcConfigState;
use store::RequestRow;

use crate::database::Connection;
use crate::lifecycle::DeviceLifecycle;
use crate::plans::store as plan_store;
use crate::plans::{Action, Plan};

/// Requests recorded for a device, as shown to operators.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceRequests {
    pub rediscover_requested: bool,
    /// BMC network settings the last automatic `configure_bmc` plan was created for.
    pub bmc_config_attempted: Option<Value>,
    /// Whether the desired BMC config differs from the detected BMC, and if so whether the
    /// automatic reconfiguration already ran. Computed here so clients don't repeat the rule.
    pub bmc_config_state: BmcConfigState,
    /// The plan most recently started for a request, whatever its outcome.
    pub last_request_plan: Option<Plan>,
    /// The device's active plan. With no active lifecycle transition it is a request or manual
    /// plan, which `POST /ui/devices/{uuid}/plan/cancel` can stop.
    pub active_plan: Option<Plan>,
}

/// Why a rediscovery request was refused.
#[derive(Debug)]
pub enum RediscoverError {
    NotFound(Uuid),
    /// The device's lifecycle state, and what to do instead.
    WrongLifecycle(&'static str, &'static str),
    Database(anyhow::Error),
}

impl From<anyhow::Error> for RediscoverError {
    fn from(e: anyhow::Error) -> Self {
        RediscoverError::Database(e)
    }
}

impl std::fmt::Display for RediscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RediscoverError::NotFound(uuid) => write!(f, "Device {uuid} not found"),
            RediscoverError::WrongLifecycle(state, hint) => {
                write!(f, "A {state} device can't be rediscovered; {hint}")
            }
            RediscoverError::Database(e) => write!(f, "{e}"),
        }
    }
}

/// Where a device's desired BMC config stands, and the settings to apply when it is `Pending`.
fn bmc_state(row: &RequestRow) -> (BmcConfigState, Option<Value>) {
    bmc::assess(
        row.bmc.as_ref(),
        row.bmc_config.as_ref(),
        row.bmc_config_attempted.as_ref(),
    )
}

impl Director<'_> {
    /// Ask for a hardware rescan. It runs the next time the device boots or polls with no
    /// active plan; there is no power kick, so a Provisioned host keeps running until rebooted.
    pub async fn request_rediscovery(&self, uuid: &Uuid) -> Result<(), RediscoverError> {
        match crate::lifecycle::store::get_device_lifecycle(self.conn, uuid).await? {
            None => Err(RediscoverError::NotFound(*uuid)),
            Some(DeviceLifecycle::New) => Err(RediscoverError::WrongLifecycle(
                "new",
                "start the Discover transition instead",
            )),
            Some(DeviceLifecycle::Removed) => Err(RediscoverError::WrongLifecycle(
                "removed",
                "it is decommissioned",
            )),
            Some(_) => Ok(store::set_rediscover_requested(self.conn, uuid).await?),
        }
    }

    pub async fn get_device_requests(&self, uuid: &Uuid) -> anyhow::Result<DeviceRequests> {
        let row = RequestRow::load(self.conn, uuid).await?;
        let last_request_plan = match row.last_request_plan_id {
            Some(id) => plan_store::get_plan(self.conn, id).await?,
            None => None,
        };
        Ok(DeviceRequests {
            rediscover_requested: row.rediscover_requested,
            bmc_config_state: bmc_state(&row).0,
            bmc_config_attempted: row.bmc_config_attempted,
            last_request_plan,
            active_plan: plan_store::get_active_plan_for_device(self.conn, uuid).await?,
        })
    }

    /// Forget the BMC settings tried automatically, so the next idle boot or poll tries them
    /// again. An operator saving BMC settings is asking for exactly that.
    pub async fn clear_bmc_config_attempt(&self, uuid: &Uuid) -> anyhow::Result<()> {
        store::clear_bmc_config_attempted(self.conn, uuid).await
    }

    /// The device's active plan or, when it has none, a plan started for an outstanding request.
    ///
    /// A request that can't be started is logged and skipped: it must not stop the device from
    /// booting or idling, and it stays recorded for the next attempt.
    pub async fn active_or_requested_plan(&mut self, uuid: &Uuid) -> anyhow::Result<Option<Plan>> {
        if let Some(plan) = plan_store::get_active_plan_for_device(self.conn, uuid).await? {
            return Ok(Some(plan));
        }
        Ok(self.start_requested_plan(uuid).await.unwrap_or_else(|e| {
            log::warn!("Couldn't start requested plan for device {uuid}: {e:#}");
            None
        }))
    }

    /// Turn the device's outstanding requests into a plan, if it has no active plan.
    ///
    /// Creating the plan and consuming the requests happen in one transaction, so a boot and
    /// a poll racing each other can't create two plans or lose a request. Idle daemons poll
    /// often, so the transaction is only opened when there is something to start.
    async fn start_requested_plan(&mut self, uuid: &Uuid) -> anyhow::Result<Option<Plan>> {
        if pending_request(self.conn, uuid).await?.is_none() {
            return Ok(None);
        }
        self.conn
            .in_transaction(async |conn| create_requested_plan(conn, uuid).await)
            .await
    }
}

/// After a scan, forget the attempted BMC settings if the detected BMC now matches the desired
/// config. Otherwise the guard would outlive the problem it was set for, and a later drift back
/// to the same settings (a factory reset, say) would never be corrected.
pub(super) async fn forget_satisfied_bmc_attempt(
    conn: &Connection,
    uuid: &Uuid,
) -> anyhow::Result<()> {
    let row = RequestRow::load(conn, uuid).await?;
    if row.bmc_config_attempted.is_some() && bmc_state(&row).0 == BmcConfigState::InSync {
        store::clear_bmc_config_attempted(conn, uuid).await?;
    }
    Ok(())
}

/// The actions to run for the device's requests, and the BMC settings they apply (if any).
struct PendingRequest {
    actions: Vec<Action>,
    bmc_target: Option<Value>,
}

/// The device's outstanding request, or `None` when there is nothing to do. Doesn't check for
/// an active plan.
async fn pending_request(conn: &Connection, uuid: &Uuid) -> anyhow::Result<Option<PendingRequest>> {
    let row = RequestRow::load(conn, uuid).await?;
    let bmc_target = bmc_state(&row).1;
    let actions = requested_actions(
        &row.lifecycle,
        row.rediscover_requested,
        bmc_target.is_some(),
    );
    Ok((!actions.is_empty()).then_some(PendingRequest {
        actions,
        bmc_target,
    }))
}

/// The in-transaction half of [`Director::start_requested_plan`]: create the plan and consume
/// the requests it covers.
async fn create_requested_plan(conn: &Connection, uuid: &Uuid) -> anyhow::Result<Option<Plan>> {
    // Re-read under the lock: a racing boot or poll may have started it already.
    if plan_store::get_active_plan_for_device(conn, uuid)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let Some(request) = pending_request(conn, uuid).await? else {
        return Ok(None);
    };

    let mut plan = Plan::new(*uuid, request.actions);
    let plan_id = plan_store::create_plan(conn, &plan).await?;
    plan.id = Some(plan_id);
    store::consume_requests(conn, uuid, request.bmc_target.as_ref(), plan_id).await?;

    log::info!(
        "Device {uuid}: started requested plan {plan_id} ({:?})",
        plan.actions
    );
    Ok(Some(plan))
}

/// The plan for a device's requests, or empty for none.
///
/// New devices get BMC config and discovery from the Discover transition, and Removed ones are
/// out of service, so neither gets a requested plan. A Provisioned device needs no trailing
/// reboot: `/cnc/poll` sends an idle Provisioned agent back to local disk, after success or
/// failure alike (a trailing `reboot_device` action would cover only success).
fn requested_actions(
    lifecycle: &DeviceLifecycle,
    rediscover: bool,
    bmc_pending: bool,
) -> Vec<Action> {
    if matches!(lifecycle, DeviceLifecycle::New | DeviceLifecycle::Removed) {
        return vec![];
    }
    match (bmc_pending, rediscover) {
        // The rescan after configure_bmc also refreshes the detected BMC state.
        (true, _) => vec![Action::ConfigureBmc, Action::DiscoverHardware],
        (false, true) => vec![Action::DiscoverHardware],
        (false, false) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::bmc::tests::{bmc, config};
    use super::*;
    use crate::director::Architecture;
    use crate::plans::PlanStatus;
    use crate::{database, test_connection_factory};
    use common::device_attributes::{BmcConfig, BmcInfo};

    #[test]
    fn test_requested_actions() {
        use DeviceLifecycle::*;
        assert!(requested_actions(&Unprovisioned, false, false).is_empty());
        assert!(requested_actions(&New, true, true).is_empty());
        assert!(requested_actions(&Removed, true, true).is_empty());
        assert_eq!(
            requested_actions(&Unprovisioned, true, false),
            vec![Action::DiscoverHardware]
        );
        assert_eq!(
            requested_actions(&Broken, true, true),
            vec![Action::ConfigureBmc, Action::DiscoverHardware]
        );
        assert_eq!(
            requested_actions(&Provisioned, false, true),
            vec![Action::ConfigureBmc, Action::DiscoverHardware]
        );
    }

    async fn device_in(
        factory: database::DatabaseConnectionFactory,
        lifecycle: DeviceLifecycle,
    ) -> (database::Connection, Uuid) {
        let mut conn = database::run_migrations(&factory).await.unwrap();
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-4466554400b1").unwrap();
        Director::new(&mut conn)
            .register_device(&uuid, Architecture::X86_64)
            .await
            .unwrap();
        crate::lifecycle::store::update_device_lifecycle(&conn, &uuid, lifecycle)
            .await
            .unwrap();
        (conn, uuid)
    }

    async fn set_bmc(conn: &mut database::Connection, uuid: &Uuid, bmc: BmcInfo, cfg: BmcConfig) {
        let mut attrs = serde_json::Map::new();
        attrs.insert("bmc".into(), serde_json::to_value(bmc).unwrap());
        attrs.insert("bmc_config".into(), serde_json::to_value(cfg).unwrap());
        Director::new(conn)
            .update_attributes(uuid, attrs)
            .await
            .unwrap();
    }

    async fn requests(conn: &mut database::Connection, uuid: &Uuid) -> DeviceRequests {
        Director::new(conn).get_device_requests(uuid).await.unwrap()
    }

    async fn finish_active_plan(conn: &database::Connection, status: &str) {
        conn.execute(
            "UPDATE plans SET status = ?1, error_message = 'scan failed'
             WHERE status IN ('pending', 'running')",
            (status.to_string(),),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_rediscovery_request_starts_plan_once() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        Director::new(&mut conn)
            .request_rediscovery(&uuid)
            .await
            .unwrap();
        assert!(requests(&mut conn, &uuid).await.rediscover_requested);

        let mut director = Director::new(&mut conn);
        let plan = director.start_requested_plan(&uuid).await.unwrap().unwrap();
        assert_eq!(plan.actions, vec![Action::DiscoverHardware]);
        // The plan is now active, and the request was consumed.
        assert!(
            director
                .start_requested_plan(&uuid)
                .await
                .unwrap()
                .is_none()
        );

        let after = requests(&mut conn, &uuid).await;
        assert!(!after.rediscover_requested);
        assert_eq!(after.last_request_plan.unwrap().id, plan.id);
        assert_eq!(after.active_plan.unwrap().id, plan.id);
    }

    #[tokio::test]
    async fn test_request_waits_for_active_plan() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        let mut director = Director::new(&mut conn);
        director
            .create_plan(&Plan::new(uuid, vec![Action::Console]))
            .await
            .unwrap();
        director.request_rediscovery(&uuid).await.unwrap();

        assert!(
            director
                .start_requested_plan(&uuid)
                .await
                .unwrap()
                .is_none()
        );
        // The active plan comes back as-is, and the request is still waiting.
        let active = director.active_or_requested_plan(&uuid).await.unwrap();
        assert_eq!(active.unwrap().actions, vec![Action::Console]);
        assert!(requests(&mut conn, &uuid).await.rediscover_requested);
    }

    #[tokio::test]
    async fn test_failed_request_plan_is_not_retried_and_is_reported() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        let mut director = Director::new(&mut conn);
        director.request_rediscovery(&uuid).await.unwrap();
        director.active_or_requested_plan(&uuid).await.unwrap();

        finish_active_plan(&conn, "failed").await;
        let mut director = Director::new(&mut conn);
        assert!(
            director
                .active_or_requested_plan(&uuid)
                .await
                .unwrap()
                .is_none(),
            "a failed request plan must not be started again"
        );

        let last = requests(&mut conn, &uuid).await.last_request_plan.unwrap();
        assert_eq!(last.status, PlanStatus::Failed);
        assert_eq!(last.error_message.as_deref(), Some("scan failed"));
    }

    #[tokio::test]
    async fn test_rediscovery_refused_for_new_removed_and_unknown_devices() {
        let (mut conn, uuid) = device_in(test_connection_factory!(), DeviceLifecycle::New).await;
        let err = Director::new(&mut conn)
            .request_rediscovery(&uuid)
            .await
            .unwrap_err();
        assert!(matches!(err, RediscoverError::WrongLifecycle("new", _)));

        crate::lifecycle::store::update_device_lifecycle(&conn, &uuid, DeviceLifecycle::Removed)
            .await
            .unwrap();
        let err = Director::new(&mut conn)
            .request_rediscovery(&uuid)
            .await
            .unwrap_err();
        assert!(matches!(err, RediscoverError::WrongLifecycle("removed", _)));
        assert!(!requests(&mut conn, &uuid).await.rediscover_requested);

        let missing = Uuid::parse_str("550e8400-e29b-41d4-a716-4466554400b2").unwrap();
        let err = Director::new(&mut conn)
            .request_rediscovery(&missing)
            .await
            .unwrap_err();
        assert!(matches!(err, RediscoverError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_pending_bmc_config_boots_into_plan_once() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Provisioned).await;
        set_bmc(
            &mut conn,
            &uuid,
            bmc("DHCP Address", Some("10.0.0.9")),
            config("static", Some("10.0.0.5")),
        )
        .await;
        assert_eq!(
            requests(&mut conn, &uuid).await.bmc_config_state,
            BmcConfigState::Pending
        );

        let boot = Director::new(&mut conn)
            .next_boot_target(&uuid, 600)
            .await
            .unwrap();
        assert!(
            matches!(&boot, crate::plans::actions::BootTarget::AgentImage { action, .. } if action == "daemon"),
            "Expected agent daemon for pending BMC config, got {boot:?}"
        );
        assert_eq!(
            requests(&mut conn, &uuid).await.bmc_config_state,
            BmcConfigState::Attempted
        );

        // Finish the plan without the BMC taking the config: the next boot goes to local disk
        // rather than retrying the same setting.
        finish_active_plan(&conn, "success").await;
        let boot = Director::new(&mut conn)
            .next_boot_target(&uuid, 600)
            .await
            .unwrap();
        assert!(matches!(boot, crate::plans::actions::BootTarget::LocalDisk));
    }

    #[tokio::test]
    async fn test_bmc_request_also_consumes_rediscovery() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        set_bmc(
            &mut conn,
            &uuid,
            bmc("DHCP Address", None),
            config("static", Some("10.0.0.5")),
        )
        .await;
        let mut director = Director::new(&mut conn);
        director.request_rediscovery(&uuid).await.unwrap();

        let plan = director.start_requested_plan(&uuid).await.unwrap().unwrap();
        assert_eq!(
            plan.actions,
            vec![Action::ConfigureBmc, Action::DiscoverHardware]
        );
        assert!(!requests(&mut conn, &uuid).await.rediscover_requested);
    }

    #[tokio::test]
    async fn test_matching_scan_clears_attempt_so_later_drift_is_corrected() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        let desired = config("static", Some("10.0.0.5"));
        set_bmc(&mut conn, &uuid, bmc("DHCP Address", None), desired.clone()).await;
        Director::new(&mut conn)
            .active_or_requested_plan(&uuid)
            .await
            .unwrap();
        finish_active_plan(&conn, "success").await;
        assert!(
            requests(&mut conn, &uuid)
                .await
                .bmc_config_attempted
                .is_some()
        );

        // The rescan sees the BMC at the desired address: the attempt is forgotten.
        set_bmc(
            &mut conn,
            &uuid,
            bmc("Static Address", Some("10.0.0.5")),
            desired.clone(),
        )
        .await;
        assert_eq!(requests(&mut conn, &uuid).await.bmc_config_attempted, None);

        // A factory reset puts it back on DHCP; the same settings are applied again.
        set_bmc(&mut conn, &uuid, bmc("DHCP Address", None), desired).await;
        assert_eq!(
            requests(&mut conn, &uuid).await.bmc_config_state,
            BmcConfigState::Pending
        );
    }

    #[tokio::test]
    async fn test_scan_that_still_differs_keeps_attempt() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        let desired = config("static", Some("10.0.0.5"));
        set_bmc(&mut conn, &uuid, bmc("DHCP Address", None), desired.clone()).await;
        Director::new(&mut conn)
            .active_or_requested_plan(&uuid)
            .await
            .unwrap();

        set_bmc(&mut conn, &uuid, bmc("DHCP Address", None), desired).await;
        assert_eq!(
            requests(&mut conn, &uuid).await.bmc_config_state,
            BmcConfigState::Attempted
        );

        // Saving settings from the UI clears the guard so they're retried.
        Director::new(&mut conn)
            .clear_bmc_config_attempt(&uuid)
            .await
            .unwrap();
        assert_eq!(
            requests(&mut conn, &uuid).await.bmc_config_state,
            BmcConfigState::Pending
        );
    }

    #[tokio::test]
    async fn test_unparseable_attempt_is_treated_as_absent() {
        let (mut conn, uuid) =
            device_in(test_connection_factory!(), DeviceLifecycle::Unprovisioned).await;
        conn.execute(
            "UPDATE devices SET bmc_config_attempted = 'not json' WHERE uuid = ?1",
            (uuid,),
        )
        .await
        .unwrap();
        assert_eq!(requests(&mut conn, &uuid).await.bmc_config_attempted, None);
    }
}
