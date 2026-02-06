use std::sync::Arc;

use crate::lifecycle::{DeviceLifecycle, LifecycleTransition};
use anyhow::Result;
use rusqlite::{Connection, params};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct LifecycleStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl LifecycleStore {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { conn }
    }

    pub async fn get_device_lifecycle(
        &self,
        device_uuid: &Uuid,
    ) -> Result<Option<DeviceLifecycle>> {
        let conn = self.conn.lock().await;
        self.get_device_lifecycle_internal(&conn, device_uuid)
    }

    pub async fn update_device_lifecycle(
        &self,
        device_uuid: &Uuid,
        lifecycle: DeviceLifecycle,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        self.update_device_lifecycle_internal(&conn, device_uuid, lifecycle)
    }

    pub async fn create_transition(&self, transition: &LifecycleTransition) -> Result<i64> {
        let conn = self.conn.lock().await;
        self.create_transition_internal(&conn, transition)
    }

    pub async fn get_active_transition_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> Result<Option<LifecycleTransition>> {
        let conn = self.conn.lock().await;
        self.get_active_transition_for_device_internal(&conn, device_uuid)
    }

    pub async fn complete_transition(
        &self,
        transition_id: i64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        self.complete_transition_internal(&conn, transition_id, success, error_message)
    }

    pub async fn get_transitions_for_device(
        &self,
        device_uuid: &Uuid,
        include_completed: bool,
    ) -> Result<Vec<LifecycleTransition>> {
        let conn = self.conn.lock().await;
        self.get_transitions_for_device_internal(&conn, device_uuid, include_completed)
    }

    pub async fn get_transition_by_plan_id(
        &self,
        plan_id: i64,
    ) -> Result<Option<LifecycleTransition>> {
        let conn = self.conn.lock().await;
        self.get_transition_by_plan_id_internal(&conn, plan_id)
    }

    fn get_device_lifecycle_internal(
        &self,
        conn: &Connection,
        device_uuid: &Uuid,
    ) -> Result<Option<DeviceLifecycle>> {
        let mut stmt = conn.prepare("SELECT lifecycle FROM devices WHERE uuid = ?1")?;

        let mut rows = stmt.query_map([device_uuid], |row| {
            let lifecycle_str: String = row.get(0)?;
            Ok(DeviceLifecycle::from(lifecycle_str))
        })?;

        if let Some(lifecycle_result) = rows.next() {
            return Ok(Some(lifecycle_result?));
        }

        Ok(None)
    }

    fn update_device_lifecycle_internal(
        &self,
        conn: &Connection,
        device_uuid: &Uuid,
        lifecycle: DeviceLifecycle,
    ) -> Result<()> {
        let lifecycle_str: String = lifecycle.into();

        conn.execute(
            "UPDATE devices SET lifecycle = ?1 WHERE uuid = ?2",
            params![&lifecycle_str, device_uuid],
        )?;

        Ok(())
    }

    fn create_transition_internal(
        &self,
        conn: &Connection,
        transition: &LifecycleTransition,
    ) -> Result<i64> {
        let from_state_str: String = transition.from_state.clone().into();
        let to_state_str: String = transition.to_state.clone().into();

        conn.execute(
            "INSERT INTO lifecycle_transitions (device_uuid, from_state, to_state, plan_id, created_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
            rusqlite::params![
                transition.device_uuid,
                from_state_str,
                to_state_str,
                transition.plan_id
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    fn get_active_transition_for_device_internal(
        &self,
        conn: &Connection,
        device_uuid: &Uuid,
    ) -> Result<Option<LifecycleTransition>> {
        let transition = crate::database::query_optional::<LifecycleTransition>(
            conn,
            "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
             FROM lifecycle_transitions
             WHERE device_uuid = ?1 AND success IS NULL
             ORDER BY created_at DESC
             LIMIT 1",
            &[device_uuid],
        )?;

        Ok(transition)
    }

    fn complete_transition_internal(
        &self,
        conn: &Connection,
        transition_id: i64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE lifecycle_transitions SET success = ?1, error_message = ?2, completed_at = CURRENT_TIMESTAMP WHERE id = ?3",
            rusqlite::params![success, error_message, transition_id],
        )?;

        Ok(())
    }

    fn get_transitions_for_device_internal(
        &self,
        conn: &Connection,
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

        let transitions = crate::database::query_map_all::<LifecycleTransition>(
            conn,
            query,
            &[device_uuid],
        )?;

        Ok(transitions)
    }

    fn get_transition_by_plan_id_internal(
        &self,
        conn: &Connection,
        plan_id: i64,
    ) -> Result<Option<LifecycleTransition>> {
        let transition = crate::database::query_optional::<LifecycleTransition>(
            conn,
            "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message
             FROM lifecycle_transitions
             WHERE plan_id = ?1",
            &[&plan_id],
        )?;

        Ok(transition)
    }
}
