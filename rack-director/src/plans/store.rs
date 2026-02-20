use std::sync::Arc;

use crate::database::{Connection, FromRow};
use crate::plans::{Plan, PlanStatus};
use anyhow::Result;
use rusqlite::OptionalExtension;
use uuid::Uuid;

#[derive(Clone)]
pub struct PlansStore {
    db: Arc<Connection>,
}

impl PlansStore {
    pub fn new(db: Arc<Connection>) -> Self {
        Self { db }
    }

    pub async fn create_plan(&self, plan: &Plan) -> Result<i64> {
        let actions_json = serde_json::to_string(&plan.actions)?;
        let status_str: String = plan.status.clone().into();

        // Check if there's already an active plan for this device
        if self
            .get_active_plan_for_device(&plan.device_uuid)
            .await?
            .is_some()
        {
            return Err(anyhow::anyhow!(
                "Cannot create new plan for device {}: an active plan already exists",
                plan.device_uuid
            ));
        }

        self.db
            .execute(
                "INSERT INTO plans (device_uuid, status, current_step, total_steps, actions, error_message, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)",
                (
                    plan.device_uuid,
                    status_str,
                    plan.current_step,
                    plan.total_steps,
                    actions_json,
                    plan.error_message.clone(),
                ),
            )
            .await?;

        Ok(self.db.last_insert_rowid().await)
    }

    pub async fn get_active_plan_for_device(&self, device_uuid: &Uuid) -> Result<Option<Plan>> {
        let plan = self
            .db
            .query_row(
                "SELECT id, device_uuid, status, current_step, total_steps, actions, error_message,
                        created_at, started_at, completed_at
                 FROM plans
                 WHERE device_uuid = ?1 AND status IN ('pending', 'running')
                 ORDER BY created_at DESC
                 LIMIT 1",
                (*device_uuid,),
                Plan::from_row,
            )
            .await
            .optional()?;

        Ok(plan)
    }

    pub async fn update_plan_status(
        &self,
        plan_id: i64,
        status: PlanStatus,
        current_step: i32,
        error_message: Option<&str>,
    ) -> Result<()> {
        let status_str: String = status.clone().into();
        let now = chrono::Utc::now();
        let error_owned = error_message.map(|s| s.to_string());

        match status {
            PlanStatus::Running => {
                self.db
                    .execute(
                        "UPDATE plans SET status = ?1, current_step = ?2, started_at = ?3 WHERE id = ?4",
                        (status_str, current_step, now, plan_id),
                    )
                    .await?;
            }
            PlanStatus::Success | PlanStatus::Failed => {
                self.db
                    .execute(
                        "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3, completed_at = ?4 WHERE id = ?5",
                        (status_str, current_step, error_owned, now, plan_id),
                    )
                    .await?;
            }
            _ => {
                self.db
                    .execute(
                        "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3 WHERE id = ?4",
                        (status_str, current_step, error_owned, plan_id),
                    )
                    .await?;
            }
        }

        Ok(())
    }
}
