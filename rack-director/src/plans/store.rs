use std::sync::Arc;

use crate::plans::{Action, Plan, PlanStatus};
use anyhow::Result;
use rusqlite::Connection;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct PlansStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl PlansStore {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { conn }
    }

    pub async fn create_plan(&self, plan: &Plan) -> Result<i64> {
        let conn = self.conn.lock().await;
        self.create_plan_internal(&conn, plan)
    }

    pub async fn get_active_plan_for_device(&self, device_uuid: &Uuid) -> Result<Option<Plan>> {
        let conn = self.conn.lock().await;
        self.get_active_plan_for_device_internal(&conn, device_uuid)
    }

    pub async fn update_plan_status(
        &self,
        plan_id: i64,
        status: PlanStatus,
        current_step: i32,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        self.update_plan_status_internal(&conn, plan_id, status, current_step, error_message)
    }

    fn create_plan_internal(&self, conn: &Connection, plan: &Plan) -> Result<i64> {
        let actions_json = serde_json::to_string(&plan.actions)?;
        let status_str: String = plan.status.clone().into();

        // Check if there's already an active plan for this device
        if let Some(_existing_plan) =
            self.get_active_plan_for_device_internal(conn, &plan.device_uuid)?
        {
            return Err(anyhow::anyhow!(
                "Cannot create new plan for device {}: an active plan already exists",
                plan.device_uuid
            ));
        }

        conn.execute(
            "INSERT INTO plans (device_uuid, status, current_step, total_steps, actions, error_message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)",
            rusqlite::params![
                plan.device_uuid,
                status_str,
                plan.current_step,
                plan.total_steps,
                actions_json,
                plan.error_message
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    fn get_active_plan_for_device_internal(
        &self,
        conn: &Connection,
        device_uuid: &Uuid,
    ) -> Result<Option<Plan>> {
        let mut stmt = conn.prepare(
            "SELECT id, device_uuid, status, current_step, total_steps, actions, error_message,
                    created_at, started_at, completed_at
             FROM plans
             WHERE device_uuid = ?1 AND status IN ('pending', 'running')
             ORDER BY created_at DESC
             LIMIT 1",
        )?;

        let mut plan_iter = stmt.query_map([device_uuid], |row| {
            let actions_json: String = row.get(5)?;
            let actions: Vec<Action> = serde_json::from_str(&actions_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

            let status_str: String = row.get(2)?;
            let status = PlanStatus::from(status_str);

            Ok(Plan {
                id: Some(row.get(0)?),
                device_uuid: row.get(1)?,
                status,
                current_step: row.get(3)?,
                total_steps: row.get(4)?,
                actions,
                error_message: row.get(6)?,
                created_at: row.get(7)?,
                started_at: row.get(8)?,
                completed_at: row.get(9)?,
            })
        })?;

        if let Some(plan_result) = plan_iter.next() {
            return Ok(Some(plan_result?));
        }

        Ok(None)
    }

    fn update_plan_status_internal(
        &self,
        conn: &Connection,
        plan_id: i64,
        status: PlanStatus,
        current_step: i32,
        error_message: Option<&str>,
    ) -> Result<()> {
        let status_str: String = status.clone().into();
        let now = chrono::Utc::now();

        match status {
            PlanStatus::Running => {
                conn.execute(
                    "UPDATE plans SET status = ?1, current_step = ?2, started_at = ?3 WHERE id = ?4",
                    rusqlite::params![status_str, current_step, now, plan_id],
                )?;
            }
            PlanStatus::Success | PlanStatus::Failed => {
                conn.execute(
                    "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3, completed_at = ?4 WHERE id = ?5",
                    rusqlite::params![status_str, current_step, error_message, now, plan_id],
                )?;
            }
            _ => {
                conn.execute(
                    "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3 WHERE id = ?4",
                    rusqlite::params![status_str, current_step, error_message, plan_id],
                )?;
            }
        }

        Ok(())
    }
}
