use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub mod store;
pub use store::PlansStore;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Pending,
    Running,
    Success,
    Failed,
}

impl From<String> for PlanStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "pending" => PlanStatus::Pending,
            "running" => PlanStatus::Running,
            "success" => PlanStatus::Success,
            "failed" => PlanStatus::Failed,
            _ => PlanStatus::Pending,
        }
    }
}

impl From<PlanStatus> for String {
    fn from(status: PlanStatus) -> Self {
        match status {
            PlanStatus::Pending => "pending".to_string(),
            PlanStatus::Running => "running".to_string(),
            PlanStatus::Success => "success".to_string(),
            PlanStatus::Failed => "failed".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub action_type: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub description: Option<String>,
}

impl Action {
    pub fn new(action_type: String, parameters: HashMap<String, serde_json::Value>) -> Self {
        Action {
            action_type,
            parameters,
            description: None,
        }
    }

    #[allow(unused)]
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Option<i64>,
    pub device_uuid: Uuid,
    pub status: PlanStatus,
    pub current_step: i32,
    pub total_steps: i32,
    pub actions: Vec<Action>,
    pub error_message: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl Plan {
    pub fn new(device_uuid: Uuid, actions: Vec<Action>) -> Self {
        Plan {
            id: None,
            device_uuid,
            status: PlanStatus::Pending,
            current_step: 0,
            total_steps: actions.len() as i32,
            actions,
            error_message: None,
            created_at: None,
            started_at: None,
            completed_at: None,
        }
    }

    pub fn get_current_action(&self) -> Option<&Action> {
        if self.current_step >= 0 && (self.current_step as usize) < self.actions.len() {
            Some(&self.actions[self.current_step as usize])
        } else {
            None
        }
    }

    #[allow(unused)]
    pub fn is_completed(&self) -> bool {
        matches!(self.status, PlanStatus::Success | PlanStatus::Failed)
    }

    #[allow(unused)]
    pub fn is_active(&self) -> bool {
        matches!(self.status, PlanStatus::Pending | PlanStatus::Running)
    }

    pub fn advance_step(&mut self) -> bool {
        if self.current_step < self.total_steps - 1 {
            self.current_step += 1;
            true
        } else {
            false
        }
    }

    pub fn mark_action_success(&mut self) -> ActionResult {
        if self.advance_step() {
            ActionResult::Continue
        } else {
            self.status = PlanStatus::Success;
            self.completed_at = Some(Utc::now());
            ActionResult::PlanCompleted
        }
    }

    pub fn mark_action_failed(&mut self, error_message: String) -> ActionResult {
        self.status = PlanStatus::Failed;
        self.error_message = Some(error_message);
        self.completed_at = Some(Utc::now());
        ActionResult::PlanFailed
    }

    pub fn start(&mut self) {
        if self.status == PlanStatus::Pending {
            self.status = PlanStatus::Running;
            self.started_at = Some(Utc::now());
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionResult {
    Continue,
    PlanCompleted,
    PlanFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_plan_creation() {
        let actions = vec![
            Action::new("install_os".to_string(), HashMap::new()),
            Action::new("configure_network".to_string(), HashMap::new()),
        ];
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let plan = Plan::new(uuid, actions);

        assert_eq!(plan.device_uuid, uuid);
        assert_eq!(plan.status, PlanStatus::Pending);
        assert_eq!(plan.current_step, 0);
        assert_eq!(plan.total_steps, 2);
        assert!(!plan.is_completed());
        assert!(plan.is_active());
    }

    #[test]
    fn test_plan_execution_flow() {
        let actions = vec![
            Action::new("step1".to_string(), HashMap::new()),
            Action::new("step2".to_string(), HashMap::new()),
        ];
        let mut plan = Plan::new(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            actions,
        );

        // Start the plan
        plan.start();
        assert_eq!(plan.status, PlanStatus::Running);

        // Complete first action
        let result = plan.mark_action_success();
        assert_eq!(result, ActionResult::Continue);
        assert_eq!(plan.current_step, 1);
        assert_eq!(plan.status, PlanStatus::Running);

        // Complete second action
        let result = plan.mark_action_success();
        assert_eq!(result, ActionResult::PlanCompleted);
        assert_eq!(plan.status, PlanStatus::Success);
        assert!(plan.is_completed());
    }

    #[test]
    fn test_plan_failure() {
        let actions = vec![Action::new("failing_action".to_string(), HashMap::new())];
        let mut plan = Plan::new(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            actions,
        );

        plan.start();
        let result = plan.mark_action_failed("Something went wrong".to_string());

        assert_eq!(result, ActionResult::PlanFailed);
        assert_eq!(plan.status, PlanStatus::Failed);
        assert_eq!(plan.error_message, Some("Something went wrong".to_string()));
        assert!(plan.is_completed());
    }

    #[test]
    fn test_get_current_action() {
        let actions = vec![
            Action::new("first".to_string(), HashMap::new()),
            Action::new("second".to_string(), HashMap::new()),
        ];
        let plan = Plan::new(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            actions,
        );

        let current = plan.get_current_action();
        assert!(current.is_some());
        assert_eq!(current.unwrap().action_type, "first");
    }
}
