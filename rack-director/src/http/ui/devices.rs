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

use super::validation::{
    ValidationErrors, validate_hostname, validate_mac_address, validate_required,
};

use crate::{
    device_warnings,
    director::{Architecture, Director},
    http::{AppState, error::Error as HttpError},
    lifecycle::{DeviceLifecycle, LifecycleTransition},
    plans::{Action, Plan},
    platforms::{AssignPlatformRequest, Platform},
    roles::{AssignRoleRequest, Role},
};
use std::net::Ipv4Addr;

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

/// Validate BMC configuration fields
///
/// Ensures that:
/// - ip_address_source is either "dhcp" or "static"
/// - For static configurations, ip_address, netmask, and gateway are provided
/// - All IP addresses are valid IPv4 addresses
fn validate_bmc_config(
    bmc_config: &serde_json::Value,
) -> Result<(), std::collections::HashMap<String, String>> {
    let mut errors = std::collections::HashMap::new();

    let obj = match bmc_config.as_object() {
        Some(o) => o,
        None => {
            errors.insert(
                "bmc_config".to_string(),
                "BMC config must be an object".to_string(),
            );
            return Err(errors);
        }
    };

    // Validate ip_address_source
    let ip_source = match obj.get("ip_address_source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            errors.insert(
                "bmc_config.ip_address_source".to_string(),
                "IP address source is required".to_string(),
            );
            return Err(errors);
        }
    };

    if ip_source != "dhcp" && ip_source != "static" {
        errors.insert(
            "bmc_config.ip_address_source".to_string(),
            "IP address source must be either 'dhcp' or 'static'".to_string(),
        );
    }

    // For static configurations, validate required fields
    if ip_source == "static" {
        // Validate IP address
        if let Some(ip_str) = obj.get("ip_address").and_then(|v| v.as_str()) {
            if ip_str.parse::<Ipv4Addr>().is_err() {
                errors.insert(
                    "bmc_config.ip_address".to_string(),
                    format!("'{}' is not a valid IPv4 address", ip_str),
                );
            }
        } else {
            errors.insert(
                "bmc_config.ip_address".to_string(),
                "IP address is required for static configuration".to_string(),
            );
        }

        // Validate netmask
        if let Some(netmask_str) = obj.get("netmask").and_then(|v| v.as_str()) {
            if let Ok(netmask) = netmask_str.parse::<Ipv4Addr>() {
                // Validate that netmask is actually a valid subnet mask
                if !is_valid_netmask(netmask) {
                    errors.insert(
                        "bmc_config.netmask".to_string(),
                        format!("'{}' is not a valid subnet mask", netmask_str),
                    );
                }
            } else {
                errors.insert(
                    "bmc_config.netmask".to_string(),
                    format!("'{}' is not a valid IPv4 address", netmask_str),
                );
            }
        } else {
            errors.insert(
                "bmc_config.netmask".to_string(),
                "Netmask is required for static configuration".to_string(),
            );
        }

        // Validate gateway
        if let Some(gateway_str) = obj.get("gateway").and_then(|v| v.as_str()) {
            if gateway_str.parse::<Ipv4Addr>().is_err() {
                errors.insert(
                    "bmc_config.gateway".to_string(),
                    format!("'{}' is not a valid IPv4 address", gateway_str),
                );
            }
        } else {
            errors.insert(
                "bmc_config.gateway".to_string(),
                "Gateway is required for static configuration".to_string(),
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check if an IPv4 address is a valid subnet mask
///
/// A valid subnet mask has all 1 bits followed by all 0 bits
/// (e.g., 255.255.255.0 = 11111111.11111111.11111111.00000000)
fn is_valid_netmask(netmask: Ipv4Addr) -> bool {
    let bits = u32::from(netmask);

    // A valid netmask has contiguous 1s followed by contiguous 0s
    // This means (bits & -bits) should equal the complement of bits + 1
    // Or more simply: bits should equal (!bits + 1) | bits

    // Check if it's a valid netmask by ensuring that when we invert and add 1,
    // we get a power of 2
    let inverted = !bits;
    let incremented = inverted.wrapping_add(1);

    // Check if incremented is a power of 2 (only one bit set)
    incremented != 0 && (incremented & (incremented.wrapping_sub(1))) == 0
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
pub(super) struct ErrorResponse {
    pub(super) error: String,
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
    platform_id: Option<i64>,
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

#[derive(Deserialize)]
struct DevicesPlanRequest {
    plan: Vec<Action>,
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
            "/ui/devices/{uuid}/lifecycle/cancel",
            post(cancel_lifecycle_transition),
        )
        .route(
            "/ui/devices/{uuid}/transitions",
            get(get_device_transitions),
        )
        .route(
            "/ui/devices/{uuid}/platform",
            post(assign_platform).get(get_device_platform),
        )
        .route(
            "/ui/devices/{uuid}/role",
            post(assign_role).get(get_device_role),
        )
        .route(
            "/ui/devices/{uuid}/transitions/active",
            get(get_active_transition),
        )
        .route("/ui/devices/{uuid}/status", get(get_device_status))
        .route("/ui/devices/pending", post(create_pending_device))
        .route("/ui/devices/pending", get(get_pending_devices))
        .route("/ui/devices/pending/{id}", delete(delete_pending_device))
        .route("/ui/devices/{uuid}/warnings", get(get_warnings))
        .route(
            "/ui/devices/{uuid}/warnings/{warning_id}",
            delete(delete_warning),
        )
        .route("/ui/devices/{uuid}/plan", post(post_device_plan))
        .with_state(state)
}

async fn get_all_devices(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DevicesIndex>, StatusCode> {
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);
    // Fetch all devices from Director (single source of truth)
    let devices = match director.get_all_devices().await {
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
                platform_id: device.platform_id,
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
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);
    // Get device from Director (single source of truth)
    let device = match director.get_device(&uuid).await {
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
        platform_id: device.platform_id,
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

    // Validate and reject username/password fields in bmc_config
    if let Some(bmc_config) = payload.attributes.get("bmc_config") {
        // First check for username/password (security)
        if let Some(obj) = bmc_config.as_object()
            && (obj.contains_key("username") || obj.contains_key("password"))
        {
            let mut errors = std::collections::HashMap::new();
            errors.insert(
                "bmc_config".to_string(),
                "BMC username and password should not be provided via the UI API".to_string(),
            );
            return Err(HttpError::ValidationError(errors));
        }

        // Then validate IP addresses and configuration
        if let Err(errors) = validate_bmc_config(bmc_config) {
            return Err(HttpError::ValidationError(errors));
        }
    }

    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(HttpError::ServerInternalError)?;
    let director = Director::new(&conn);

    // Update device attributes
    match director.update_attributes(&uuid, payload.attributes).await {
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
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);
    match director.get_device_lifecycle(&uuid).await {
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

    let conn = state.connection_factory.open().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;
    let director = Director::with_power_config(&conn, state.power_config);

    match director.start_lifecycle_transition(&uuid, to_state).await {
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

async fn cancel_lifecycle_transition(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.connection_factory.open().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;
    let director = Director::new(&conn);

    match director.cancel_active_transition(&uuid).await {
        Ok(()) => Ok(Json(serde_json::json!({}))),
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
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);

    match director
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
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);
    match director.get_active_transition_for_device(&uuid).await {
        Ok(transition) => Ok(Json(transition)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_device_status(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<DeviceStatusResponse>, StatusCode> {
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let director = Director::new(&conn);

    let current_lifecycle = match director.get_device_lifecycle(&uuid).await {
        Ok(lifecycle) => lifecycle.map(String::from),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let active_transition = match director.get_active_transition_for_device(&uuid).await {
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
    extract::Json(mut payload): extract::Json<CreatePendingDeviceRequest>,
) -> Result<(StatusCode, Json<PendingDeviceResponse>), HttpError> {
    // Normalize MAC address to lowercase for consistent storage and duplicate detection
    payload.mac_address = payload.mac_address.to_lowercase();

    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    // Validate MAC address format and check for duplicates/network existence
    validate_create_pending_device(&conn, &director, &payload).await?;

    // Create the pending device
    let id = director
        .create_pending_device(&payload.mac_address, payload.network_id)
        .await?;

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

/// Validate a create-pending-device request.
///
/// Checks MAC address format, network existence, and absence of a duplicate
/// pending device for the same MAC address. Returns a `ValidationError` when
/// any check fails so the handler can return a structured 400 response.
async fn validate_create_pending_device(
    conn: &crate::database::Connection,
    director: &Director<'_>,
    payload: &CreatePendingDeviceRequest,
) -> Result<(), HttpError> {
    let mut errors = ValidationErrors::new();

    // Sync format checks — guard format validation so "is required" isn't overwritten
    errors.add_if_err(
        "mac_address",
        validate_required(&payload.mac_address, "MAC address"),
    );
    if !payload.mac_address.is_empty() {
        errors.add_if_err("mac_address", validate_mac_address(&payload.mac_address));
    }

    // Network existence check
    if crate::dhcp::store::get_network(conn, payload.network_id)
        .await
        .is_err()
    {
        errors.add_error("network_id", "Network not found".to_string());
    }

    // Duplicate pending device check
    match director
        .find_pending_device_by_mac(&payload.mac_address)
        .await
    {
        Ok(Some(_)) => {
            errors.add_error(
                "mac_address",
                "A pending device with this MAC address already exists".to_string(),
            );
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("Failed to check for duplicate pending device: {}", e);
        }
    }

    errors.into_result().map_err(HttpError::ValidationError)
}

async fn get_pending_devices(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PendingDeviceResponse>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);
    let devices = director.get_pending_devices().await?;
    let responses = devices
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

async fn delete_pending_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);
    director.delete_pending_device(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_device_by_uuid(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.connection_factory.open().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;
    let director = Director::new(&conn);
    match director.delete_device(&uuid).await {
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

async fn post_device_plan(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    extract::Json(payload): extract::Json<DevicesPlanRequest>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    let plan = Plan::new(uuid, payload.plan);
    director.create_plan(&plan).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }

    use crate::{
        database, database::DatabaseConnectionFactory, director::Director, storage::ImageStore,
        test_connection_factory,
    };

    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    async fn setup_test_state(
        factory: DatabaseConnectionFactory,
    ) -> (
        Arc<AppState>,
        tempfile::TempDir,
        crate::database::Connection,
    ) {
        // Create image store for testing
        let store = ImageStore::new(crate::storage::ImageStoreConfig::Memory {}).unwrap();

        // Create temporary directories needed for agent images and boot files
        let temp_dir = tempdir().unwrap();

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

        let conn: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        // Run migrations and retain the connection so the in-memory DB persists for the test
        let migration_conn = database::run_migrations(conn.as_ref()).await.unwrap();

        let state = Arc::new(AppState {
            connection_factory: conn,
            image_store: store.into(),
            agent_images_path,
            boot_file_provider,
            dhcp: crate::dhcp::DhcpControl::noop(),
            unprovisioned_sleep_secs: 600,
            bundled_osm_path: None,
            power_config: crate::director::power::PowerConfig::default(),
        });
        (state, temp_dir, migration_conn)
    }

    /// Open a database connection for test assertions.
    ///
    /// Usage: `let db = test_db(&state).await; let director = Director::new(&db);`
    /// For one-liners: `{ let db = test_db(&state).await; Director::new(&db).method().await }`
    async fn test_db(state: &AppState) -> crate::database::Connection {
        state.connection_factory.open().await.unwrap()
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(state: &AppState) -> i64 {
        let db = Arc::new(state.connection_factory.open().await.unwrap());
        let network = crate::dhcp::store::create_network(
            &db,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(&db, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_get_device_lifecycle() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x10);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/ui/devices/{}/lifecycle", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_start_lifecycle_transition() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x11);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state);

        let payload = StartTransitionRequest {
            to_state: "unprovisioned".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri(format!("/ui/devices/{}/lifecycle/transition", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_device_status() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x12);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/ui/devices/{}/status", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete_pending_device() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let network_id = create_test_network(&state).await;

        // Create a pending device directly (bypassing network/lease setup for simplicity)
        let mac = "aa:bb:cc:dd:ee:ff";

        let pending_id = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .create_pending_device(mac, network_id)
                .await
                .unwrap()
        };

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
        let pending_devices = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_pending_devices().await.unwrap()
        };
        assert!(
            pending_devices.is_empty(),
            "Pending device should be deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_device() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x20);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        // Verify device exists before deletion
        assert!(
            {
                let conn = test_db(&state).await;
                Director::new(&conn)
                    .device_exists(&test_uuid)
                    .await
                    .unwrap()
            },
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
            !{
                let conn = test_db(&state).await;
                Director::new(&conn)
                    .device_exists(&test_uuid)
                    .await
                    .unwrap()
            },
            "Device should be deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_multiple_devices() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let uuid1 = test_uuid(0x21);
        let uuid2 = test_uuid(0x22);

        // Register two devices
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&uuid1, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&uuid2, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        assert!(!{
            let conn = test_db(&state).await;
            Director::new(&conn).device_exists(&uuid1).await.unwrap()
        });
        assert!(!{
            let conn = test_db(&state).await;
            Director::new(&conn).device_exists(&uuid2).await.unwrap()
        });
    }

    #[tokio::test]
    async fn test_delete_nonexistent_device() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x30);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert_eq!(device.attributes.hostname, Some("server-01".to_string()));
    }

    #[tokio::test]
    async fn test_update_device_hostname_empty() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x31);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x32);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x33);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x34);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x35);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x36);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert_eq!(
            device.attributes.hostname,
            Some("server.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_update_device_hostname_not_string() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x37);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x38);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert_eq!(
            device.attributes.manufacturer,
            Some("Dell Inc.".to_string())
        );
    }

    #[tokio::test]
    async fn test_update_bmc_config_rejects_password() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x39);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Try to update bmc_config with password (should be rejected)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "password": "secret123"
            }),
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
        assert!(body_str.contains("bmc_config"));
        assert!(body_str.contains("username") || body_str.contains("password"));
    }

    #[tokio::test]
    async fn test_update_bmc_config_rejects_username() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3A);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Try to update bmc_config with username (should be rejected)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "dhcp",
                "username": "admin"
            }),
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
        assert!(body_str.contains("bmc_config"));
        assert!(body_str.contains("username") || body_str.contains("password"));
    }

    #[tokio::test]
    async fn test_update_bmc_config_allows_valid_fields() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3B);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Update bmc_config with valid fields only (should succeed)
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "netmask": "255.255.255.0",
                "gateway": "10.0.1.1"
            }),
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

        // Verify bmc_config was updated
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        let bmc_config = device
            .attributes
            .bmc_config
            .expect("bmc_config should be set");
        assert_eq!(bmc_config.ip_address_source, "static");
        assert_eq!(bmc_config.ip_address, Some("10.0.1.100".parse().unwrap()));
    }

    // ========== BMC IP Validation Tests ==========

    #[tokio::test]
    async fn test_update_bmc_config_invalid_ip_address() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3C);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "300.400.500.600",  // Invalid IP
                "netmask": "255.255.255.0",
                "gateway": "10.0.1.1"
            }),
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

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("ip_address"));
        assert!(body_str.contains("valid"));
    }

    #[tokio::test]
    async fn test_update_bmc_config_invalid_netmask() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3D);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.1.100",
                "netmask": "255.255.255.3",  // Invalid netmask (not contiguous bits)
                "gateway": "10.0.1.1"
            }),
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

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("netmask"));
        assert!(body_str.contains("subnet mask"));
    }

    #[tokio::test]
    async fn test_update_bmc_config_missing_static_fields() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3E);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static"
                // Missing ip_address, netmask, gateway
            }),
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

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("required"));
    }

    #[tokio::test]
    async fn test_update_bmc_config_dhcp_no_validation() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x3F);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // DHCP mode should not require IP fields
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "dhcp"
            }),
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

        // Verify bmc_config was updated
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        let bmc_config = device
            .attributes
            .bmc_config
            .expect("bmc_config should be set");
        assert_eq!(bmc_config.ip_address_source, "dhcp");
        assert!(bmc_config.ip_address.is_none());
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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x50);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .update_attributes(&test_uuid, attributes)
                .await
                .unwrap();
        }

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
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x51);

        // Register device
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

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

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .update_attributes(&test_uuid, attributes)
                .await
                .unwrap();
        }

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

    #[tokio::test]
    async fn test_create_pending_device_success() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let network_id = create_test_network(&state).await;

        let app = routes(state.clone());

        // Create a pending device — no lease required
        let payload = serde_json::json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id
        });
        let request = Request::builder()
            .method("POST")
            .uri("/ui/devices/pending")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_pending_device_invalid_mac() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let network_id = create_test_network(&state).await;

        let app = routes(state.clone());

        let payload = serde_json::json!({
            "mac_address": "not-a-mac",
            "network_id": network_id
        });
        let request = Request::builder()
            .method("POST")
            .uri("/ui/devices/pending")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_pending_device_unknown_network() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;

        let app = routes(state.clone());

        let payload = serde_json::json!({
            "mac_address": "aa:bb:cc:dd:ee:02",
            "network_id": 9999_i64
        });
        let request = Request::builder()
            .method("POST")
            .uri("/ui/devices/pending")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_pending_device_duplicate_mac() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let network_id = create_test_network(&state).await;
        let mac = "aa:bb:cc:dd:ee:03";

        // Pre-create a pending device directly
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .create_pending_device(mac, network_id)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        let payload = serde_json::json!({
            "mac_address": mac,
            "network_id": network_id
        });
        let request = Request::builder()
            .method("POST")
            .uri("/ui/devices/pending")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_validate_mac_address_valid() {
        use super::super::validation::validate_mac_address;
        assert!(validate_mac_address("aa:bb:cc:dd:ee:ff").is_none());
        assert!(validate_mac_address("AA:BB:CC:DD:EE:FF").is_none());
        assert!(validate_mac_address("00:11:22:33:44:55").is_none());
    }

    #[test]
    fn test_validate_mac_address_invalid() {
        use super::super::validation::validate_mac_address;
        // Wrong number of octets
        assert!(validate_mac_address("aa:bb:cc:dd:ee").is_some());
        // Non-hex characters
        assert!(validate_mac_address("gg:bb:cc:dd:ee:ff").is_some());
        // Wrong separator
        assert!(validate_mac_address("aa-bb-cc-dd-ee-ff").is_some());
        // Octet too long
        assert!(validate_mac_address("aaa:bb:cc:dd:ee:ff").is_some());
        // Empty string
        assert!(validate_mac_address("").is_some());
    }

}

// Platform assignment handlers

async fn assign_platform(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    Json(req): Json<AssignPlatformRequest>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    // Verify platform exists
    crate::platforms::store::get(&conn, req.platform_id).await?;

    // Verify device exists
    director.get_device(&uuid).await?;

    director
        .assign_platform_to_device(&uuid, req.platform_id)
        .await?;

    Ok(StatusCode::OK)
}

async fn get_device_platform(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<Option<Platform>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    // Get platform ID from device
    let platform_id = director.get_device_platform_id(&uuid).await?;

    // If device has a platform, fetch full platform details
    if let Some(id) = platform_id {
        let platform = crate::platforms::store::get(&conn, id).await?;
        Ok(Json(Some(platform)))
    } else {
        Ok(Json(None))
    }
}

// Role assignment handlers

async fn assign_role(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    Json(req): Json<AssignRoleRequest>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    // Verify role exists
    crate::roles::store::get(&conn, req.role_id).await?;

    // Verify device exists
    director.get_device(&uuid).await?;

    director.assign_role_to_device(&uuid, req.role_id).await?;

    Ok(StatusCode::OK)
}

async fn get_device_role(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<Option<Role>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

    // Get role ID from device
    let role_id = director.get_device_role_id(&uuid).await?;

    // If device has a role, fetch full role details
    if let Some(id) = role_id {
        let role = crate::roles::store::get(&conn, id).await?;
        Ok(Json(Some(role)))
    } else {
        Ok(Json(None))
    }
}

/// `GET /ui/devices/{uuid}/warnings`
///
/// List all warnings for the device.
async fn get_warnings(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<Vec<device_warnings::DeviceWarning>>, HttpError> {
    let conn = state.connection_factory.open().await?;

    let device_id = device_warnings::get_device_id_by_uuid(&conn, &uuid)
        .await?
        .ok_or_else(|| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    let warnings = device_warnings::list_warnings(&conn, device_id).await?;
    Ok(Json(warnings))
}

/// `DELETE /ui/devices/{uuid}/warnings/{warning_id}`
///
/// Dismiss (delete) a single warning by its numeric ID.
///
/// Returns `204 No Content` on success, `404` if the device or warning is not found.
async fn delete_warning(
    State(state): State<Arc<AppState>>,
    Path((uuid, warning_id)): Path<(Uuid, i64)>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;

    let device_id = device_warnings::get_device_id_by_uuid(&conn, &uuid)
        .await?
        .ok_or_else(|| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    let deleted = device_warnings::delete_warning(&conn, warning_id, device_id).await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::NotFound(format!(
            "Warning {} not found on device {}",
            warning_id, uuid
        )))
    }
}
