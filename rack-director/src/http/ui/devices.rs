use std::sync::Arc;

use axum::{
    Router,
    extract::{self, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, patch, post},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    http::AppState,
    lifecycle::{DeviceLifecycle, LifecycleTransition},
    operating_systems::Architecture,
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

#[derive(Deserialize, Serialize)]
struct CreatePendingDeviceRequest {
    mac_address: String,
    network_id: i64,
}

#[derive(Deserialize)]
struct UpdateAttributesRequest {
    attributes: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize)]
struct PendingDeviceResponse {
    id: i64,
    mac_address: String,
    device_uuid: Option<String>,
    network_id: i64,
    created_at: String,
    completed_at: Option<String>,
}

#[derive(Serialize)]
struct DeviceResponse {
    uuid: String,
    architecture: Architecture,
    lifecycle: Option<DeviceLifecycle>,
    role_id: Option<i64>,
    attributes: serde_json::Map<String, serde_json::Value>,
    created_at: Option<String>,
    first_seen_at: Option<String>,
    last_seen_at: Option<String>,
    ip_address: Option<String>,
    mac_address: Option<String>,
    hostname: Option<String>,
}

#[derive(Serialize)]
struct DevicesIndex {
    devices: Vec<DeviceResponse>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/devices", get(get_all_devices))
        .route("/ui/devices/{uuid}", get(get_device_by_uuid))
        .route(
            "/ui/devices/{uuid}/attributes",
            patch(update_device_attributes),
        )
        .route("/ui/devices/{uuid}/lifecycle", get(get_device_lifecycle))
        .route(
            "/ui/devices/{uuid}/lifecycle/transition",
            post(start_lifecycle_transition),
        )
        .route(
            "/ui/devices/{uuid}/transitions",
            get(get_device_transitions),
        )
        .route(
            "/ui/devices/{uuid}/transitions/active",
            get(get_active_transition),
        )
        .route("/ui/devices/{uuid}/status", get(get_device_status))
        .route("/ui/devices/pending", post(create_pending_device))
        .route("/ui/devices/pending", get(get_pending_devices))
        .route("/ui/devices/pending/{id}", delete(delete_pending_device))
        .with_state(state)
}

async fn get_all_devices(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DevicesIndex>, StatusCode> {
    // Fetch all devices from Director (single source of truth)
    let devices = match state.director.get_all_devices().await {
        Ok(devices) => devices,
        Err(e) => {
            log::error!("Failed to fetch devices: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Build responses from device attributes only
    let device_responses: Vec<DeviceResponse> = devices
        .into_iter()
        .map(|device| {
            // Extract all network info from device attributes
            let hostname = device
                .attributes
                .get("hostname")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mac_address = device
                .attributes
                .get("mac_address")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let ip_address = device
                .attributes
                .get("ip_address")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            DeviceResponse {
                uuid: device.uuid.to_string(),
                architecture: device.architecture,
                lifecycle: device.lifecycle,
                role_id: device.role_id,
                attributes: device.attributes,
                created_at: device.created_at,
                first_seen_at: device.first_seen_at,
                last_seen_at: device.last_seen_at,
                ip_address,
                mac_address,
                hostname,
            }
        })
        .collect();

    Ok(Json(DevicesIndex {
        devices: device_responses,
    }))
}

async fn get_device_by_uuid(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<DeviceResponse>, StatusCode> {
    // Get device from Director (single source of truth)
    let device = match state.director.get_device(&uuid).await {
        Ok(device) => device,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // Extract all info from device attributes
    let hostname = device
        .attributes
        .get("hostname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mac_address = device
        .attributes
        .get("mac_address")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let ip_address = device
        .attributes
        .get("ip_address")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(Json(DeviceResponse {
        uuid: device.uuid.to_string(),
        architecture: device.architecture,
        lifecycle: device.lifecycle,
        role_id: device.role_id,
        attributes: device.attributes,
        created_at: device.created_at,
        first_seen_at: device.first_seen_at,
        last_seen_at: device.last_seen_at,
        ip_address,
        mac_address,
        hostname,
    }))
}

async fn update_device_attributes(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    extract::Json(payload): extract::Json<UpdateAttributesRequest>,
) -> Result<StatusCode, StatusCode> {
    // Update device attributes
    match state
        .director
        .update_attributes(&uuid, payload.attributes)
        .await
    {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            log::error!("Failed to update device attributes for {}: {}", uuid, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_device_lifecycle(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<DeviceLifecycle>, StatusCode> {
    match state.director.get_device_lifecycle(&uuid).await {
        Ok(Some(lifecycle)) => Ok(Json(lifecycle)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn start_lifecycle_transition(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
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
    Path(uuid): Path<Uuid>,
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
    Path(uuid): Path<Uuid>,
) -> Result<Json<Option<LifecycleTransition>>, StatusCode> {
    match state.director.get_active_transition_for_device(&uuid).await {
        Ok(transition) => Ok(Json(transition)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_device_status(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
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
        device_uuid: uuid.to_string(),
        current_lifecycle,
        active_transition,
    }))
}

async fn create_pending_device(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<CreatePendingDeviceRequest>,
) -> Result<(StatusCode, Json<PendingDeviceResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Validate that the lease exists by MAC address
    let lease = match state
        .dhcp_store
        .get_lease_by_mac(&payload.mac_address)
        .await
    {
        Ok(Some(lease)) => lease,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!(
                        "No DHCP lease found for MAC address {}",
                        payload.mac_address
                    ),
                }),
            ));
        }
        Err(e) => {
            log::error!("Failed to query DHCP lease: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to query DHCP lease".to_string(),
                }),
            ));
        }
    };

    // Check that the lease is active
    if lease.state != crate::dhcp::LeaseState::Active {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!(
                    "Lease for MAC {} is not active (state: {:?})",
                    payload.mac_address, lease.state
                ),
            }),
        ));
    }

    // Check that the lease doesn't already have a device
    if lease.device_uuid.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!(
                    "Lease for MAC {} already has a device UUID",
                    payload.mac_address
                ),
            }),
        ));
    }

    // Create the pending device
    let id = match state
        .director
        .create_pending_device(&payload.mac_address, payload.network_id)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            log::error!("Failed to create pending device: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to create pending device".to_string(),
                }),
            ));
        }
    };

    // Return the created pending device
    Ok((
        StatusCode::CREATED,
        Json(PendingDeviceResponse {
            id,
            mac_address: payload.mac_address,
            device_uuid: None,
            network_id: payload.network_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        }),
    ))
}

async fn get_pending_devices(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PendingDeviceResponse>>, StatusCode> {
    match state.director.get_pending_devices().await {
        Ok(devices) => {
            let responses: Vec<PendingDeviceResponse> = devices
                .into_iter()
                .map(|d| PendingDeviceResponse {
                    id: d.id,
                    mac_address: d.mac_address,
                    device_uuid: d.device_uuid.map(|u| u.to_string()),
                    network_id: d.network_id,
                    created_at: d.created_at,
                    completed_at: d.completed_at,
                })
                .collect();
            Ok(Json(responses))
        }
        Err(e) => {
            log::error!("Failed to get pending devices: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn delete_pending_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.director.delete_pending_device(id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            log::error!("Failed to delete pending device {}: {}", id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to delete pending device".to_string(),
                }),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }
    use crate::{database, director::Director, storage::MemoryImageStore};

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

        // Create agent-image directory with mock files for testing
        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();
        std::fs::write(agent_images_path.join("vmlinuz"), b"mock kernel data").unwrap();
        std::fs::write(
            agent_images_path.join("initramfs.img"),
            b"mock initramfs data",
        )
        .unwrap();

        let state = Arc::new(AppState {
            director: Director::new(
                db_tokio.clone(),
                Arc::new(MemoryImageStore::new()),
                "http://localhost:8080",
            ),
            dhcp_store: crate::dhcp::DhcpStore::new(db_tokio.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db_tokio.clone()),
            roles_store: crate::roles::RolesStore::new(db_tokio),
            agent_images_path,
        });
        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_get_device_lifecycle() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x10);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/ui/devices/{}/lifecycle", test_uuid.to_string()))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_start_lifecycle_transition() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x11);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let payload = StartTransitionRequest {
            to_state: "unprovisioned".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/ui/devices/{}/lifecycle/transition",
                test_uuid.to_string()
            ))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_device_status() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x12);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/ui/devices/{}/status", test_uuid.to_string()))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete_pending_device() {
        let (state, _temp_dir) = setup_test_state().await;

        // Create a pending device directly (bypassing network/lease setup for simplicity)
        let mac = "aa:bb:cc:dd:ee:ff";
        let network_id = 1;

        let pending_id = state
            .director
            .create_pending_device(mac, network_id)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Delete the pending device
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/ui/devices/pending/{}", pending_id))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify it's deleted - should return empty list
        let pending_devices = state.director.get_pending_devices().await.unwrap();
        assert!(
            pending_devices.is_empty(),
            "Pending device should be deleted"
        );
    }
}
