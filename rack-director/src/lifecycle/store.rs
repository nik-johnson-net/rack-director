use crate::lifecycle::{DeviceLifecycle, LifecycleTransition};
use anyhow::Result;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::database::{Connection, FromRow};

pub async fn get_device_lifecycle(
    conn: &Connection,
    device_uuid: &Uuid,
) -> Result<Option<DeviceLifecycle>> {
    let result = conn
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
    conn: &Connection,
    device_uuid: &Uuid,
    lifecycle: DeviceLifecycle,
) -> Result<()> {
    let lifecycle_str: String = lifecycle.into();
    conn.execute(
        "UPDATE devices SET lifecycle = ?1 WHERE uuid = ?2",
        (lifecycle_str, *device_uuid),
    )
    .await?;

    Ok(())
}

pub async fn create_transition(conn: &Connection, transition: &LifecycleTransition) -> Result<i64> {
    let from_state_str: String = transition.from_state.clone().into();
    let to_state_str: String = transition.to_state.clone().into();

    conn.execute(
        "INSERT INTO lifecycle_transitions (device_uuid, from_state, to_state, plan_id, created_at)
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
        (
            transition.device_uuid,
            from_state_str,
            to_state_str,
            transition.plan_id,
        ),
    )
    .await?;

    Ok(conn.last_insert_rowid().await)
}

pub async fn get_active_transition_for_device(
    conn: &Connection,
    device_uuid: &Uuid,
) -> Result<Option<LifecycleTransition>> {
    let transition = conn
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

/// Mark a lifecycle transition as complete. Only updates rows where `success IS NULL`
/// (i.e., still open), making this safe to call multiple times. Returns the number of
/// rows updated (0 if the transition was already closed by a concurrent path).
pub async fn complete_transition(
    conn: &Connection,
    transition_id: i64,
    success: bool,
    error_message: Option<&str>,
) -> Result<usize> {
    let rows = conn
        .execute(
            "UPDATE lifecycle_transitions SET success = ?1, error_message = ?2, completed_at = CURRENT_TIMESTAMP
             WHERE id = ?3 AND success IS NULL",
            (success, error_message.map(|s| s.to_string()), transition_id),
        )
        .await?;

    Ok(rows)
}

pub async fn get_transitions_for_device(
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

    let transitions = conn
        .query(query, (*device_uuid,), LifecycleTransition::from_row)
        .await?;

    Ok(transitions)
}

pub async fn get_transition_by_plan_id(
    conn: &Connection,
    plan_id: i64,
) -> Result<Option<LifecycleTransition>> {
    let transition = conn
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
