use std::sync::Arc;

use crate::lifecycle::{DeviceLifecycle, LifecycleTransition};
use anyhow::Result;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::database::{Connection, FromRow};

#[derive(Clone)]
pub struct LifecycleStore {
    db: Arc<Connection>,
}

impl LifecycleStore {
    pub fn new(db: Arc<Connection>) -> Self {
        Self { db }
    }

    pub async fn get_device_lifecycle(
        &self,
        device_uuid: &Uuid,
    ) -> Result<Option<DeviceLifecycle>> {
        let result = self
            .db
            .query_row(
                "SELECT lifecycle FROM devices WHERE uuid = ?1",
                (*device_uuid,),
                |row| {
                    let lifecycle_str: String = row.get(0)?;
                    Ok(DeviceLifecycle::from(lifecycle_str))
                },
            )
            .await
            .optional()?;

        Ok(result)
    }

    pub async fn update_device_lifecycle(
        &self,
        device_uuid: &Uuid,
        lifecycle: DeviceLifecycle,
    ) -> Result<()> {
        let lifecycle_str: String = lifecycle.into();
        self.db
            .execute(
                "UPDATE devices SET lifecycle = ?1 WHERE uuid = ?2",
                (lifecycle_str, *device_uuid),
            )
            .await?;

        Ok(())
    }

    pub async fn create_transition(&self, transition: &LifecycleTransition) -> Result<i64> {
        let from_state_str: String = transition.from_state.clone().into();
        let to_state_str: String = transition.to_state.clone().into();

        self.db
            .execute(
                "INSERT INTO lifecycle_transitions (device_uuid, from_state, to_state, plan_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
                (transition.device_uuid, from_state_str, to_state_str, transition.plan_id),
            )
            .await?;

        Ok(self.db.last_insert_rowid().await)
    }

    pub async fn get_active_transition_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> Result<Option<LifecycleTransition>> {
        let transition = self
            .db
            .query_row(
                "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
                 FROM lifecycle_transitions
                 WHERE device_uuid = ?1 AND success IS NULL
                 ORDER BY created_at DESC
                 LIMIT 1",
                (*device_uuid,),
                LifecycleTransition::from_row,
            )
            .await
            .optional()?;

        Ok(transition)
    }

    pub async fn complete_transition(
        &self,
        transition_id: i64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE lifecycle_transitions SET success = ?1, error_message = ?2, completed_at = CURRENT_TIMESTAMP WHERE id = ?3",
                (success, error_message.map(|s| s.to_string()), transition_id),
            )
            .await?;

        Ok(())
    }

    pub async fn get_transitions_for_device(
        &self,
        device_uuid: &Uuid,
        include_completed: bool,
    ) -> Result<Vec<LifecycleTransition>> {
        let query = if include_completed {
            "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
             FROM lifecycle_transitions
             WHERE device_uuid = ?1
             ORDER BY created_at DESC"
        } else {
            "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
             FROM lifecycle_transitions
             WHERE device_uuid = ?1 AND success IS NULL
             ORDER BY created_at DESC"
        };

        let transitions = self
            .db
            .query(query, (*device_uuid,), LifecycleTransition::from_row)
            .await?;

        Ok(transitions)
    }

    pub async fn get_transition_by_plan_id(
        &self,
        plan_id: i64,
    ) -> Result<Option<LifecycleTransition>> {
        let transition = self
            .db
            .query_row(
                "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
                 FROM lifecycle_transitions
                 WHERE plan_id = ?1",
                (plan_id,),
                LifecycleTransition::from_row,
            )
            .await
            .optional()?;

        Ok(transition)
    }
}
