use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    http::{AppState, error::Error},
    plans::{self, actions::Action},
};
use common::poll_action::{PollAction, PollResponse};

/// Query parameters for the poll endpoint.
#[derive(Debug, Deserialize)]
pub struct PollQuery {
    pub uuid: Uuid,
}

impl From<&Action> for PollAction {
    /// Convert an internal [`Action`] to its wire representation.
    fn from(action: &Action) -> Self {
        match action {
            Action::DiscoverHardware => PollAction::DiscoverHardware,
            Action::ConfigureBmc => PollAction::ConfigureBmc,
            Action::PartitionDisks => PollAction::PartitionDisks,
            Action::RebootDevice => PollAction::RebootDevice,
            Action::InstallOs => PollAction::InstallOs,
            Action::Console => PollAction::Console,
        }
    }
}

/// Poll rack-director for a pending action for a specific device.
///
/// Returns:
/// - `200 OK` with a JSON [`PollResponse`] if the device has an active plan with a current action.
/// - `204 No Content` if the device has no active plan, or the plan has no remaining actions.
/// - `500 Internal Server Error` on database failure.
#[axum::debug_handler]
pub async fn poll_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PollQuery>,
) -> Result<Response, Error> {
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(Error::ServerInternalError)?;

    if !crate::director::store::device_exists(&conn, &params.uuid)
        .await
        .map_err(Error::ServerInternalError)?
    {
        return Err(Error::NotFound(format!("Device {} not found", params.uuid)));
    }

    // Stamp the daemon heartbeat so the power-kick logic can detect that this
    // device's agent is already running.  Non-fatal: a failure here must never
    // break polling.
    if let Err(e) = crate::director::store::update_device_last_polled(&conn, &params.uuid).await {
        log::warn!(
            "Failed to update last_polled_at for device {}: {}",
            params.uuid,
            e
        );
    }

    let plan = plans::store::get_active_plan_for_device(&conn, &params.uuid).await?;

    let Some(plan) = plan else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };

    let Some(action) = plan.get_current_action() else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };

    let payload = PollAction::from(action);
    let response = PollResponse::Action {
        payload,
        plan_id: plan.id,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database,
        plans::{Plan, store::create_plan},
        test_connection_factory,
    };
    use axum::{Router, routing::get};
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::super::test_helpers::setup_test_state;

    /// Build an Axum test router wired to the poll handler.
    fn test_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/cnc/poll", get(poll_handler))
            .with_state(state)
    }

    /// Register a device in the DB so plans can reference it by UUID.
    async fn register_device(conn: &database::Connection, uuid: Uuid) {
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_poll_returns_action_when_plan_is_active() {
        let (state, _tmp, migration_conn) = setup_test_state(test_connection_factory!()).await;

        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440010").unwrap();
        register_device(&migration_conn, uuid).await;

        let plan = Plan::new(uuid, vec![Action::DiscoverHardware, Action::ConfigureBmc]);
        create_plan(&migration_conn, &plan).await.unwrap();

        let app = test_router(state);
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/cnc/poll?uuid={}", uuid))
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Outer envelope should have type="action"
        assert_eq!(json["type"], "action");
        // Inner payload should identify the first action
        assert_eq!(json["payload"]["type"], "discover_hardware");
    }

    #[tokio::test]
    async fn test_poll_returns_204_when_no_active_plan() {
        let (state, _tmp, migration_conn) = setup_test_state(test_connection_factory!()).await;

        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440011").unwrap();
        register_device(&migration_conn, uuid).await;

        // No plan created for this device.
        let app = test_router(state);
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/cnc/poll?uuid={}", uuid))
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_poll_returns_204_when_plan_step_exhausted() {
        let (state, _tmp, migration_conn) = setup_test_state(test_connection_factory!()).await;

        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440012").unwrap();
        register_device(&migration_conn, uuid).await;

        // Create a plan whose current_step is past the end of its action list.
        // We insert directly to bypass the guard in `create_plan` and set an out-of-bounds step.
        let actions_json = r#"[{"type":"discover_hardware"}]"#;
        migration_conn
            .execute(
                "INSERT INTO plans (device_uuid, status, current_step, total_steps, actions)
                 VALUES (?1, 'running', 5, 1, ?2)",
                (uuid, actions_json),
            )
            .await
            .unwrap();

        let app = test_router(state);
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/cnc/poll?uuid={}", uuid))
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    /// Polling for an unknown device UUID must return 404.
    ///
    /// Before step 4, the handler would return 204 No Content for an unregistered
    /// device because it silently returned "no active plan". With the device existence
    /// check in place it now correctly distinguishes "device not found" (404) from
    /// "device has no active plan" (204).
    #[tokio::test]
    async fn test_poll_returns_404_for_unknown_device() {
        let (state, _tmp, _migration_conn) = setup_test_state(test_connection_factory!()).await;

        // Deliberately do NOT register a device — UUID is unknown to the DB.
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440099").unwrap();

        let app = test_router(state);
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/cnc/poll?uuid={}", uuid))
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Polling a registered device must stamp `last_polled_at` on the device row.
    #[tokio::test]
    async fn test_poll_stamps_last_polled_at() {
        let (state, _tmp, migration_conn) = setup_test_state(test_connection_factory!()).await;

        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440020").unwrap();
        register_device(&migration_conn, uuid).await;

        // Verify last_polled_at is unset before polling.
        // The column is a plain nullable DATETIME (no DEFAULT), so it should be
        // None before the first poll. We also tolerate the legacy "0"/empty
        // sentinel for robustness — none of these are a real timestamp.
        let device_before = crate::director::store::get_device(&migration_conn, &uuid)
            .await
            .unwrap();
        let polled_before = device_before.last_polled_at.clone();
        let is_real_ts = polled_before
            .as_deref()
            .map(|s| s != "0" && !s.is_empty())
            .unwrap_or(false);
        assert!(
            !is_real_ts,
            "last_polled_at should not be set before first poll, got: {:?}",
            polled_before
        );

        // Hit the poll endpoint
        let app = test_router(state);
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/cnc/poll?uuid={}", uuid))
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        // 204 (no active plan) is fine — we just care about the side-effect
        assert!(response.status() == StatusCode::NO_CONTENT || response.status() == StatusCode::OK);

        // last_polled_at must now be set to a real timestamp
        let device_after = crate::director::store::get_device(&migration_conn, &uuid)
            .await
            .unwrap();
        let polled_after = device_after.last_polled_at.as_deref().unwrap_or("0");
        assert!(
            polled_after != "0" && !polled_after.is_empty(),
            "last_polled_at should be set after poll, got: {:?}",
            device_after.last_polled_at
        );
        // Must be parseable as a real timestamp
        assert!(
            crate::director::power::is_in_daemon_mode(
                Some(polled_after),
                std::time::Duration::from_secs(15)
            ),
            "Freshly polled device should be within the daemon heartbeat window"
        );
    }

    #[test]
    fn test_poll_action_serialization() {
        let response = PollResponse::Action {
            payload: PollAction::DiscoverHardware,
            plan_id: Some(42),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            json,
            r#"{"type":"action","payload":{"type":"discover_hardware"},"plan_id":42}"#
        );
    }

    #[test]
    fn test_poll_action_serialization_no_plan_id() {
        let response = PollResponse::Action {
            payload: PollAction::DiscoverHardware,
            plan_id: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            json,
            r#"{"type":"action","payload":{"type":"discover_hardware"},"plan_id":null}"#
        );
    }

    #[test]
    fn test_from_all_action_variants() {
        assert_eq!(
            PollAction::from(&Action::DiscoverHardware),
            PollAction::DiscoverHardware
        );
        assert_eq!(
            PollAction::from(&Action::ConfigureBmc),
            PollAction::ConfigureBmc
        );
        assert_eq!(
            PollAction::from(&Action::PartitionDisks),
            PollAction::PartitionDisks
        );
        assert_eq!(
            PollAction::from(&Action::RebootDevice),
            PollAction::RebootDevice
        );
        assert_eq!(PollAction::from(&Action::InstallOs), PollAction::InstallOs);
        assert_eq!(PollAction::from(&Action::Console), PollAction::Console);
    }
}
