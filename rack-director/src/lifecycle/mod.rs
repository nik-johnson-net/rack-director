use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod store;
pub use store::LifecycleStore;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceLifecycle {
    New,
    Unprovisioned,
    Provisioned,
    Removed,
    Broken,
}

impl From<String> for DeviceLifecycle {
    fn from(s: String) -> Self {
        match s.as_str() {
            "new" => DeviceLifecycle::New,
            "unprovisioned" => DeviceLifecycle::Unprovisioned,
            "provisioned" => DeviceLifecycle::Provisioned,
            "removed" => DeviceLifecycle::Removed,
            "broken" => DeviceLifecycle::Broken,
            _ => DeviceLifecycle::New,
        }
    }
}

impl From<DeviceLifecycle> for String {
    fn from(lifecycle: DeviceLifecycle) -> Self {
        match lifecycle {
            DeviceLifecycle::New => "new".to_string(),
            DeviceLifecycle::Unprovisioned => "unprovisioned".to_string(),
            DeviceLifecycle::Provisioned => "provisioned".to_string(),
            DeviceLifecycle::Removed => "removed".to_string(),
            DeviceLifecycle::Broken => "broken".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransitionType {
    Discover,
    Provision,
    Deprovision,
    Remove,
    Repair,
}

impl From<String> for TransitionType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "discover" => TransitionType::Discover,
            "provision" => TransitionType::Provision,
            "deprovision" => TransitionType::Deprovision,
            "remove" => TransitionType::Remove,
            "repair" => TransitionType::Repair,
            _ => TransitionType::Provision,
        }
    }
}

impl From<TransitionType> for String {
    fn from(transition: TransitionType) -> Self {
        match transition {
            TransitionType::Discover => "discover".to_string(),
            TransitionType::Provision => "provision".to_string(),
            TransitionType::Deprovision => "deprovision".to_string(),
            TransitionType::Remove => "remove".to_string(),
            TransitionType::Repair => "repair".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleTransition {
    pub id: Option<i64>,
    pub device_uuid: String,
    pub from_state: DeviceLifecycle,
    pub to_state: DeviceLifecycle,
    pub plan_id: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub success: Option<bool>,
    pub error_message: Option<String>,
}

impl LifecycleTransition {
    pub fn new(
        device_uuid: String,
        from_state: DeviceLifecycle,
        to_state: DeviceLifecycle,
        plan_id: Option<i64>,
    ) -> Self {
        LifecycleTransition {
            id: None,
            device_uuid,
            from_state,
            to_state,
            plan_id,
            created_at: None,
            completed_at: None,
            success: None,
            error_message: None,
        }
    }
}

pub struct LifecycleManager;

impl LifecycleManager {
    pub fn is_transition_allowed(from: &DeviceLifecycle, to: &DeviceLifecycle) -> bool {
        use DeviceLifecycle::*;

        matches!(
            (from, to),
            // Forward transitions
            (New, Unprovisioned) |
            (Unprovisioned, Provisioned) |

            // Backward transitions
            (Provisioned, Unprovisioned) |
            (Unprovisioned, Removed) |

            // Failure transitions (to broken)
            (New, Broken) |
            (Unprovisioned, Broken) |
            (Provisioned, Broken) |

            // Repair transition
            (Broken, Unprovisioned)
        )
    }

    pub fn get_transition_type(
        from: &DeviceLifecycle,
        to: &DeviceLifecycle,
    ) -> Option<TransitionType> {
        use DeviceLifecycle::*;

        match (from, to) {
            (New, Unprovisioned) => Some(TransitionType::Discover),
            (Unprovisioned, Provisioned) => Some(TransitionType::Provision),
            (Provisioned, Unprovisioned) => Some(TransitionType::Deprovision),
            (Unprovisioned, Removed) => Some(TransitionType::Remove),
            (Broken, Unprovisioned) => Some(TransitionType::Repair),
            (_, Broken) => None, // Automatic transition on failure
            _ => None,
        }
    }

    pub fn get_plan_stub_for_transition(
        transition_type: &TransitionType,
    ) -> Vec<crate::plans::Action> {
        use crate::plans::Action;

        match transition_type {
            TransitionType::Discover => {
                vec![Action::new("discover_hardware".to_string(), HashMap::new())]
            }
            TransitionType::Provision => vec![
                Action::new("install_os".to_string(), HashMap::new()),
                Action::new("configure_network".to_string(), HashMap::new()),
                Action::new("install_software".to_string(), HashMap::new()),
            ],
            TransitionType::Deprovision => vec![
                Action::new("backup_data".to_string(), HashMap::new()),
                Action::new("remove_software".to_string(), HashMap::new()),
                Action::new("factory_reset".to_string(), HashMap::new()),
            ],
            TransitionType::Remove => vec![
                Action::new("secure_wipe".to_string(), HashMap::new()),
                Action::new("inventory_removal".to_string(), HashMap::new()),
            ],
            TransitionType::Repair => vec![
                Action::new("run_diagnostics".to_string(), HashMap::new()),
                Action::new("repair_issues".to_string(), HashMap::new()),
                Action::new("verify_functionality".to_string(), HashMap::new()),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        use DeviceLifecycle::*;

        // Forward transitions
        assert!(LifecycleManager::is_transition_allowed(
            &New,
            &Unprovisioned
        ));
        assert!(LifecycleManager::is_transition_allowed(
            &Unprovisioned,
            &Provisioned
        ));

        // Backward transitions
        assert!(LifecycleManager::is_transition_allowed(
            &Provisioned,
            &Unprovisioned
        ));
        assert!(LifecycleManager::is_transition_allowed(
            &Unprovisioned,
            &Removed
        ));

        // Failure transitions
        assert!(LifecycleManager::is_transition_allowed(&New, &Broken));
        assert!(LifecycleManager::is_transition_allowed(
            &Unprovisioned,
            &Broken
        ));
        assert!(LifecycleManager::is_transition_allowed(
            &Provisioned,
            &Broken
        ));

        // Repair transition
        assert!(LifecycleManager::is_transition_allowed(
            &Broken,
            &Unprovisioned
        ));
    }

    #[test]
    fn test_invalid_transitions() {
        use DeviceLifecycle::*;

        // Direct transitions that aren't allowed
        assert!(!LifecycleManager::is_transition_allowed(&New, &Provisioned));
        assert!(!LifecycleManager::is_transition_allowed(&New, &Removed));
        assert!(!LifecycleManager::is_transition_allowed(
            &Provisioned,
            &Removed
        ));
        assert!(!LifecycleManager::is_transition_allowed(&Removed, &New));
        assert!(!LifecycleManager::is_transition_allowed(
            &Broken,
            &Provisioned
        ));
    }

    #[test]
    fn test_transition_types() {
        use DeviceLifecycle::*;

        assert_eq!(
            LifecycleManager::get_transition_type(&New, &Unprovisioned),
            Some(TransitionType::Discover)
        );
        assert_eq!(
            LifecycleManager::get_transition_type(&Unprovisioned, &Provisioned),
            Some(TransitionType::Provision)
        );
        assert_eq!(
            LifecycleManager::get_transition_type(&Provisioned, &Unprovisioned),
            Some(TransitionType::Deprovision)
        );
        assert_eq!(
            LifecycleManager::get_transition_type(&Unprovisioned, &Removed),
            Some(TransitionType::Remove)
        );
        assert_eq!(
            LifecycleManager::get_transition_type(&Broken, &Unprovisioned),
            Some(TransitionType::Repair)
        );
    }

    #[test]
    fn test_plan_stubs() {
        let discover_actions =
            LifecycleManager::get_plan_stub_for_transition(&TransitionType::Discover);
        assert_eq!(discover_actions.len(), 1);
        assert_eq!(discover_actions[0].action_type, "discover_hardware");

        let provision_actions =
            LifecycleManager::get_plan_stub_for_transition(&TransitionType::Provision);
        assert_eq!(provision_actions.len(), 3);
        assert_eq!(provision_actions[0].action_type, "install_os");

        let deprovision_actions =
            LifecycleManager::get_plan_stub_for_transition(&TransitionType::Deprovision);
        assert_eq!(deprovision_actions.len(), 3);
        assert_eq!(deprovision_actions[0].action_type, "backup_data");
    }
}
