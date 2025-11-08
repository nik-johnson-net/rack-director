use std::sync::Arc;

use axum::{
    Router,
    extract::{self, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    http::AppState,
    lifecycle::{DeviceLifecycle, LifecycleTransition},
};

#[derive(Deserialize, Serialize)]
struct StartTransitionRequest {
    to_state: String,
}

#[derive(Serialize)]
struct StartTransitionResponse {
    transition_id: i64,
    message: String,
}

#[derive(Deserialize)]
struct DeviceTransitionsQuery {
    include_completed: Option<bool>,
}

#[derive(Serialize)]
struct DeviceStatusResponse {
    device_uuid: String,
    current_lifecycle: Option<String>,
    active_transition: Option<LifecycleTransition>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/devices/{uuid}/lifecycle", get(get_device_lifecycle))
        .route(
            "/api/devices/{uuid}/lifecycle/transition",
            post(start_lifecycle_transition),
        )
        .route(
            "/api/devices/{uuid}/transitions",
            get(get_device_transitions),
        )
        .route(
            "/api/devices/{uuid}/transitions/active",
            get(get_active_transition),
        )
        .route("/api/devices/{uuid}/status", get(get_device_status))
        .route("/api/dhcp/leases", get(get_all_dhcp_leases))
        .route("/api/dhcp/leases/{mac}", get(get_dhcp_lease_by_mac))
        .with_state(state)
}

async fn get_device_lifecycle(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
) -> Result<Json<DeviceLifecycle>, StatusCode> {
    match state.director.get_device_lifecycle(&uuid).await {
        Ok(Some(lifecycle)) => Ok(Json(lifecycle)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn start_lifecycle_transition(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
    extract::Json(payload): extract::Json<StartTransitionRequest>,
) -> Result<Json<StartTransitionResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Parse the target state
    let to_state = match payload.to_state.as_str() {
        "new" => DeviceLifecycle::New,
        "unprovisioned" => DeviceLifecycle::Unprovisioned,
        "provisioned" => DeviceLifecycle::Provisioned,
        "removed" => DeviceLifecycle::Removed,
        "broken" => DeviceLifecycle::Broken,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid lifecycle state: {}", payload.to_state),
                }),
            ));
        }
    };

    match state
        .director
        .start_lifecycle_transition(&uuid, to_state)
        .await
    {
        Ok(transition_id) => Ok(Json(StartTransitionResponse {
            transition_id,
            message: format!("Started lifecycle transition for device {}", uuid),
        })),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_device_transitions(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
    Query(params): Query<DeviceTransitionsQuery>,
) -> Result<Json<Vec<LifecycleTransition>>, StatusCode> {
    let include_completed = params.include_completed.unwrap_or(false);

    match state
        .director
        .get_device_transitions(&uuid, include_completed)
        .await
    {
        Ok(transitions) => Ok(Json(transitions)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_active_transition(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
) -> Result<Json<Option<LifecycleTransition>>, StatusCode> {
    match state.director.get_active_transition_for_device(&uuid).await {
        Ok(transition) => Ok(Json(transition)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_device_status(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
) -> Result<Json<DeviceStatusResponse>, StatusCode> {
    let current_lifecycle = match state.director.get_device_lifecycle(&uuid).await {
        Ok(lifecycle) => lifecycle.map(String::from),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let active_transition = match state.director.get_active_transition_for_device(&uuid).await {
        Ok(transition) => transition,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    Ok(Json(DeviceStatusResponse {
        device_uuid: uuid,
        current_lifecycle,
        active_transition,
    }))
}

async fn get_all_dhcp_leases(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::dhcp::Lease>>, StatusCode> {
    match state.dhcp_store.get_all_leases().await {
        Ok(leases) => Ok(Json(leases)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_dhcp_lease_by_mac(
    State(state): State<Arc<AppState>>,
    Path(mac): Path<String>,
) -> Result<Json<crate::dhcp::Lease>, StatusCode> {
    match state.dhcp_store.get_lease_by_mac(&mac).await {
        Ok(Some(lease)) => Ok(Json(lease)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(test)]
mod tests {
    use crate::{database, director::Director};

    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    async fn setup_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let db_tokio = Arc::new(tokio::sync::Mutex::new(db));

        // Create image store for testing
        let storage_path = temp_dir.path().join("images");
        let image_store = crate::storage::LocalImageStore::new(
            storage_path,
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        let state = Arc::new(AppState {
            director: Director::new(db_tokio.clone()),
            dhcp_store: crate::dhcp::DhcpStore::new(db_tokio.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db_tokio.clone()),
            roles_store: crate::roles::RolesStore::new(db_tokio),
        });
        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_get_device_lifecycle() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440010";

        // Register device
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/api/devices/{}/lifecycle", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_start_lifecycle_transition() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440011";

        // Register device
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let payload = StartTransitionRequest {
            to_state: "unprovisioned".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/devices/{}/lifecycle/transition", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_device_status() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440012";

        // Register device
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/api/devices/{}/status", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
