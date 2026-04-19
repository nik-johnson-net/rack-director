use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::FromRow;

pub mod actions;
pub mod store;

pub use actions::Action;

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

impl FromRow for Plan {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let actions_json: String = row.get("actions")?;
        let actions: Vec<Action> = serde_json::from_str(&actions_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let status_str: String = row.get("status")?;
        let status = PlanStatus::from(status_str);

        Ok(Plan {
            id: Some(row.get("id")?),
            device_uuid: row.get("device_uuid")?,
            status,
            current_step: row.get("current_step")?,
            total_steps: row.get("total_steps")?,
            actions,
            error_message: row.get("error_message")?,
            created_at: row.get("created_at")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
        })
    }
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

    fn advance_step(&mut self) -> bool {
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
            self.mark_completed();
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

            // Some transitions may have no actions. Immediately mark success
            if self.no_more_actions() {
                self.mark_completed();
            }
        }
    }

    fn no_more_actions(&self) -> bool {
        self.current_step >= self.total_steps
    }

    fn mark_completed(&mut self) {
        self.status = PlanStatus::Success;
        self.completed_at = Some(Utc::now());
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
        let actions = vec![Action::InstallOs, Action::PartitionDisks];
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
        let actions = vec![Action::DiscoverHardware, Action::ConfigureBmc];
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
        let actions = vec![Action::InstallOs];
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
        let actions = vec![Action::DiscoverHardware, Action::ConfigureBmc];
        let plan = Plan::new(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            actions,
        );

        let current = plan.get_current_action();
        assert!(current.is_some());
        assert_eq!(*current.unwrap(), Action::DiscoverHardware);
    }

    #[test]
    fn empty_plan_success_on_start() {
        let actions = vec![];
        let mut plan = Plan::new(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            actions,
        );

        plan.start();
        assert!(plan.is_completed())
    }
}
