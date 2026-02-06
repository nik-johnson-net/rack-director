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

use super::validation::validate_hostname;
use crate::{
    http::{AppState, error::Error as HttpError},
    lifecycle::{DeviceLifecycle, LifecycleTransition},
    operating_systems::Architecture,
};

/// Sanitize device attributes for UI consumption
///
/// Removes sensitive fields like BMC passwords before returning to the UI.
/// The UI should never see actual BMC passwords for security reasons.
fn sanitize_attributes_for_ui(attributes: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(bmc_config) = attributes.get_mut("bmc_config")
        && let Some(obj) = bmc_config.as_object_mut()
    {
        obj.remove("password");
    }
}

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

#[derive(Deserialize, Serialize)]
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
        .route(
            "/ui/devices/{uuid}",
            get(get_device_by_uuid).delete(delete_device_by_uuid),
        )
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
            let hostname = device.attributes.hostname.clone();
            let mac_address = device.attributes.mac_address.clone();
            let ip_address = device.attributes.static_ip.clone();

            // Serialize DeviceAttributes to JSON map for API response
            let mut attributes_json = serde_json::to_value(&device.attributes)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();

            // Sanitize sensitive fields before returning to UI
            sanitize_attributes_for_ui(&mut attributes_json);

            DeviceResponse {
                uuid: device.uuid.to_string(),
                architecture: device.architecture,
                lifecycle: device.lifecycle,
                role_id: device.role_id,
                attributes: attributes_json,
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
    let hostname = device.attributes.hostname.clone();
    let mac_address = device.attributes.mac_address.clone();
    let ip_address = device.attributes.static_ip.clone();

    // Serialize DeviceAttributes to JSON map for API response
    let mut attributes_json = serde_json::to_value(&device.attributes)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    // Sanitize sensitive fields before returning to UI
    sanitize_attributes_for_ui(&mut attributes_json);

    Ok(Json(DeviceResponse {
        uuid: device.uuid.to_string(),
        architecture: device.architecture,
        lifecycle: device.lifecycle,
        role_id: device.role_id,
        attributes: attributes_json,
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
) -> Result<StatusCode, HttpError> {
    // Validate hostname if present in attributes
    if let Some(hostname_value) = payload.attributes.get("hostname") {
        if let Some(hostname) = hostname_value.as_str() {
            // Validate the hostname
            if let Err(errors) = validate_hostname(hostname) {
                return Err(HttpError::ValidationError(errors));
            }
        } else {
            // hostname field exists but is not a string
            let mut errors = std::collections::HashMap::new();
            errors.insert(
                "hostname".to_string(),
                "Hostname must be a string".to_string(),
            );
            return Err(HttpError::ValidationError(errors));
        }
    }

    // Update device attributes
    match state
        .director
        .update_attributes(&uuid, payload.attributes)
        .await
    {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            log::error!("Failed to update device attributes for {}: {}", uuid, e);
            Err(HttpError::ServerInternalError(e))
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

async fn delete_device_by_uuid(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.director.delete_device(&uuid).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            log::error!("Failed to delete device {}: {}", uuid, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to delete device".to_string(),
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

        // Create test network (migration 12 removed the default network)
        {
            let conn = db_tokio.lock().await;
            conn.execute(
                "INSERT INTO dhcp_networks (id, name, subnet, gateway, dns_servers, lease_duration)
                 VALUES (1, 'Test Network', '10.0.0.0/24', '10.0.0.1', '[\"8.8.8.8\"]', 86400)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO dhcp_pools (network_id, name, range_start, range_end)
                 VALUES (1, 'Test Pool', '10.0.0.100', '10.0.0.200')",
                [],
            )
            .unwrap();
        }

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

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

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
            boot_file_provider,
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

    #[tokio::test]
    async fn test_delete_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x20);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        // Verify device exists before deletion
        assert!(
            state.director.device_exists(&test_uuid).await.unwrap(),
            "Device should exist before deletion"
        );

        let app = routes(state.clone());

        // Delete the device
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/ui/devices/{}", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify device is deleted
        assert!(
            !state.director.device_exists(&test_uuid).await.unwrap(),
            "Device should be deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_multiple_devices() {
        let (state, _temp_dir) = setup_test_state().await;
        let uuid1 = test_uuid(0x21);
        let uuid2 = test_uuid(0x22);

        // Register two devices
        state
            .director
            .register_device(&uuid1, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        state
            .director
            .register_device(&uuid2, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Delete first device
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/ui/devices/{}", uuid1))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Delete second device
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/ui/devices/{}", uuid2))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify both devices are deleted
        assert!(!state.director.device_exists(&uuid1).await.unwrap());
        assert!(!state.director.device_exists(&uuid2).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x22);

        // Don't register the device - it doesn't exist

        let app = routes(state.clone());

        // Try to delete non-existent device
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/ui/devices/{}", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should still return NO_CONTENT (idempotent delete)
        // SQLite DELETE with no matching rows is still successful
        assert_eq!(
            response.status(),
            StatusCode::NO_CONTENT,
            "Deleting non-existent device should be idempotent"
        );
    }

    // ========== Hostname Update Tests ==========

    #[tokio::test]
    async fn test_update_device_hostname_valid() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x30);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Update hostname
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("server-01".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify hostname was updated
        let device = state.director.get_device(&test_uuid).await.unwrap();
        assert_eq!(device.attributes.hostname, Some("server-01".to_string()));
    }

    #[tokio::test]
    async fn test_update_device_hostname_empty() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x31);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with empty hostname
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response contains validation error
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("required"));
    }

    #[tokio::test]
    async fn test_update_device_hostname_too_long() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x32);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with hostname that's too long (254 chars, exceeds 253 limit)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("a".repeat(254)),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("253"));
    }

    #[tokio::test]
    async fn test_update_device_hostname_invalid_characters() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x33);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with invalid hostname (contains underscore)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("server_01".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("letters") || body_str.contains("alphanumeric"));
    }

    #[tokio::test]
    async fn test_update_device_hostname_leading_hyphen() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x34);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with hostname starting with hyphen
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("-server".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("hyphen"));
    }

    #[tokio::test]
    async fn test_update_device_hostname_trailing_hyphen() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x35);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with hostname ending with hyphen
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("server-".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("hyphen"));
    }

    #[tokio::test]
    async fn test_update_device_hostname_with_dots() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x36);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Update hostname with dots (FQDN-style)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("server.example.com".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify hostname was updated
        let device = state.director.get_device(&test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname,
            Some("server.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_update_device_hostname_not_string() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x37);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Try to update with hostname as a number instead of string
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::Number(123.into()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify error response
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("hostname"));
        assert!(body_str.contains("string"));
    }

    #[tokio::test]
    async fn test_update_device_attributes_without_hostname() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x38);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Update other attributes without hostname (should work)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "manufacturer".to_string(),
            serde_json::Value::String("Dell Inc.".to_string()),
        );

        let payload = UpdateAttributesRequest { attributes };

        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/ui/devices/{}/attributes", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify attribute was updated
        let device = state.director.get_device(&test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.manufacturer,
            Some("Dell Inc.".to_string())
        );
    }

    // ========== Password Sanitization Tests ==========

    #[test]
    fn test_sanitize_attributes_removes_password() {
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "username": "RACKDIRECTOR",
                "password": "secret123"
            }),
        );

        sanitize_attributes_for_ui(&mut attributes);

        // Verify password is removed
        let bmc_config = attributes.get("bmc_config").unwrap().as_object().unwrap();
        assert!(bmc_config.get("password").is_none());

        // Verify other fields remain
        assert_eq!(
            bmc_config.get("username").unwrap().as_str().unwrap(),
            "RACKDIRECTOR"
        );
        assert_eq!(
            bmc_config.get("ip_address").unwrap().as_str().unwrap(),
            "10.0.1.100"
        );
    }

    #[test]
    fn test_sanitize_attributes_no_bmc_config() {
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("test".to_string()),
        );

        sanitize_attributes_for_ui(&mut attributes);

        // Should not panic, just leave attributes unchanged
        assert_eq!(
            attributes.get("hostname").unwrap().as_str().unwrap(),
            "test"
        );
    }

    #[test]
    fn test_sanitize_attributes_bmc_config_no_password() {
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "dhcp",
                "username": "admin"
            }),
        );

        sanitize_attributes_for_ui(&mut attributes);

        // Should not panic even if no password field exists
        let bmc_config = attributes.get("bmc_config").unwrap().as_object().unwrap();
        assert_eq!(
            bmc_config.get("username").unwrap().as_str().unwrap(),
            "admin"
        );
    }

    #[tokio::test]
    async fn test_get_device_sanitizes_password() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x50);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC config with password
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "username": "RACKDIRECTOR",
                "password": "supersecret"
            }),
        );

        state
            .director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Get device via UI endpoint
        let request = Request::builder()
            .uri(format!("/ui/devices/{}", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let device_response: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify password is not in response
        let bmc_config = device_response
            .get("attributes")
            .unwrap()
            .get("bmc_config")
            .unwrap();
        assert!(bmc_config.get("password").is_none());

        // Verify username is still there
        assert_eq!(
            bmc_config.get("username").unwrap().as_str().unwrap(),
            "RACKDIRECTOR"
        );
    }

    #[tokio::test]
    async fn test_get_all_devices_sanitizes_passwords() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x51);

        // Register device
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC config with password
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "username": "RACKDIRECTOR",
                "password": "anothersecret"
            }),
        );

        state
            .director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        let app = routes(state.clone());

        // Get all devices
        let request = Request::builder()
            .uri("/ui/devices")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let devices_response: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Find our device
        let devices = devices_response.get("devices").unwrap().as_array().unwrap();
        let device = devices
            .iter()
            .find(|d| d.get("uuid").unwrap().as_str().unwrap() == test_uuid.to_string())
            .unwrap();

        // Verify password is not in response
        let bmc_config = device.get("attributes").unwrap().get("bmc_config").unwrap();
        assert!(bmc_config.get("password").is_none());

        // Verify username is still there
        assert_eq!(
            bmc_config.get("username").unwrap().as_str().unwrap(),
            "RACKDIRECTOR"
        );
    }
}
