use serde::{Deserialize, Serialize};

/// Action sent from rack-director to the agent daemon via the poll endpoint.
///
/// Shared between rack-director (serialization) and rack-agent (deserialization).
/// Kept in `common` so that adding a new variant causes a compile error in both
/// crates simultaneously, preventing wire protocol drift.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PollAction {
    DiscoverHardware,
    ConfigureBmc,
    PartitionDisks,
    RebootDevice,
    InstallOs,
    Console,
}

/// Envelope wrapping a [`PollAction`] sent from rack-director to the agent daemon.
///
/// Shared between rack-director (serialization) and rack-agent (deserialization).
/// Kept in `common` alongside [`PollAction`] so the full wire contract is defined
/// in one place.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PollResponse {
    Action {
        payload: PollAction,
        /// The database ID of the active plan this action belongs to.
        ///
        /// Agents should echo this back in `action_success` / `action_failed`
        /// requests so that rack-director can reject stale reports that arrive
        /// after the plan has been cancelled and a new plan created.
        plan_id: Option<i64>,
    },
}
