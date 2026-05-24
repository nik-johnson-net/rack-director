use crate::database::{Connection, FromRow};
use crate::plans::{Plan, PlanStatus};
use anyhow::Result;
use rusqlite::OptionalExtension;
use uuid::Uuid;

pub async fn create_plan(conn: &Connection, plan: &Plan) -> Result<i64> {
    let actions_json = serde_json::to_string(&plan.actions)?;
    let status_str: String = plan.status.clone().into();

    // RebootDevice and InstallOs must never be the first action in a plan.
    //
    // RebootDevice: a daemon agent already running on the device would exit
    // immediately, causing an unnecessary reboot before any real work is done.
    // RebootDevice is valid as an intermediate step (e.g. between two actions
    // that require a clean reboot), just not as the first.
    //
    // InstallOs: the OS installer is served by rack-director only after
    // partition_disks has completed. Placing InstallOs first would skip disk
    // preparation and attempt to install onto unpartitioned disks.
    if matches!(
        plan.actions.first(),
        Some(crate::plans::Action::RebootDevice) | Some(crate::plans::Action::InstallOs)
    ) {
        return Err(anyhow::anyhow!(
            "Cannot create plan for device {}: {:?} cannot be the first action",
            plan.device_uuid,
            plan.actions.first().unwrap()
        ));
    }

    // Check if there's already an active plan for this device
    if get_active_plan_for_device(conn, &plan.device_uuid)
        .await?
        .is_some()
    {
        return Err(anyhow::anyhow!(
            "Cannot create new plan for device {}: an active plan already exists",
            plan.device_uuid
        ));
    }

    conn.execute(
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

    Ok(conn.last_insert_rowid().await)
}

pub async fn get_active_plan_for_device(
    conn: &Connection,
    device_uuid: &Uuid,
) -> Result<Option<Plan>> {
    let plan = conn
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

/// Cancel the active plan for a device using a conditional (CAS-style) UPDATE.
///
/// Only cancels if the plan is still in `pending` or `running` status, which
/// prevents a race with a concurrent `action_success` call. Returns `true` if
/// a plan was cancelled, `false` if none was found (already completed, or no
/// active plan exists).
pub async fn cancel_active_plan_for_device(conn: &Connection, device_uuid: &Uuid) -> Result<bool> {
    let now = chrono::Utc::now();
    let rows = conn
        .execute(
            "UPDATE plans SET status = 'cancelled', error_message = 'Cancelled by user', completed_at = ?1
             WHERE device_uuid = ?2 AND status IN ('pending', 'running')",
            (now, *device_uuid),
        )
        .await?;
    Ok(rows > 0)
}

pub async fn update_plan_status(
    conn: &Connection,
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
            conn.execute(
                "UPDATE plans SET status = ?1, current_step = ?2, started_at = ?3 WHERE id = ?4",
                (status_str, current_step, now, plan_id),
            )
            .await?;
        }
        PlanStatus::Success | PlanStatus::Failed | PlanStatus::Cancelled => {
            conn.execute(
                "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3, completed_at = ?4 WHERE id = ?5",
                (status_str, current_step, error_owned, now, plan_id),
            )
            .await?;
        }
        _ => {
            conn.execute(
                "UPDATE plans SET status = ?1, current_step = ?2, error_message = ?3 WHERE id = ?4",
                (status_str, current_step, error_owned, plan_id),
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::{Action, Plan};
    use crate::{database, test_connection_factory};
    use uuid::Uuid;

    async fn setup(factory: crate::database::DatabaseConnectionFactory) -> Connection {
        database::run_migrations(&factory).await.unwrap()
    }

    async fn register_device(conn: &Connection, uuid: Uuid) {
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_create_plan_rejects_reboot_device_as_first_action() {
        let conn = setup(test_connection_factory!()).await;
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655441001").unwrap();
        register_device(&conn, uuid).await;

        let plan = Plan::new(uuid, vec![Action::RebootDevice, Action::DiscoverHardware]);
        let result = create_plan(&conn, &plan).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("RebootDevice cannot be the first action")
        );
    }

    #[tokio::test]
    async fn test_create_plan_rejects_install_os_as_first_action() {
        let conn = setup(test_connection_factory!()).await;
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655441004").unwrap();
        register_device(&conn, uuid).await;

        let plan = Plan::new(uuid, vec![Action::InstallOs, Action::DiscoverHardware]);
        let result = create_plan(&conn, &plan).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("InstallOs cannot be the first action")
        );
    }

    #[tokio::test]
    async fn test_create_plan_allows_reboot_device_as_later_action() {
        let conn = setup(test_connection_factory!()).await;
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655441002").unwrap();
        register_device(&conn, uuid).await;

        // RebootDevice is valid as a non-first action
        let plan = Plan::new(
            uuid,
            vec![
                Action::DiscoverHardware,
                Action::RebootDevice,
                Action::ConfigureBmc,
            ],
        );
        let result = create_plan(&conn, &plan).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_plan_rejects_duplicate_active_plan() {
        let conn = setup(test_connection_factory!()).await;
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655441003").unwrap();
        register_device(&conn, uuid).await;

        let plan = Plan::new(uuid, vec![Action::DiscoverHardware]);
        create_plan(&conn, &plan).await.unwrap();

        let plan2 = Plan::new(uuid, vec![Action::ConfigureBmc]);
        let result = create_plan(&conn, &plan2).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("an active plan already exists")
        );
    }
}
