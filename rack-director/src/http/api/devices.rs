//! `/api/devices` HTTP handlers for device-level disk label overrides, NIC assignments, and warnings.
//!
//! These endpoints allow operators to pin platform labels to specific disk paths on a
//! per-device basis, manually assign NICs to networks with a static IP, and view or
//! dismiss warnings that the system generates automatically.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use common::Ipv4Subnet;

use crate::{
    device_warnings, dhcp,
    director::{self, Director},
    http::{AppState, error::Error as HttpError},
};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Body for `PUT /api/devices/{id}/label-overrides`.
#[derive(Deserialize)]
pub struct PutLabelOverrideRequest {
    /// The platform label to pin (e.g. `"ROOT"`, `"DATA1"`).
    pub label: String,
    /// The `/dev/disk/by-path/…` value the label should resolve to on this device.
    pub path: String,
}

/// A single label-override entry returned in list responses.
#[derive(Serialize)]
pub struct LabelOverrideEntry {
    pub label: String,
    pub path: String,
}

/// Response body for `PUT /api/devices/{id}/label-overrides`.
#[derive(Serialize)]
pub struct LabelOverridesResponse {
    pub overrides: Vec<LabelOverrideEntry>,
}

// ---------------------------------------------------------------------------
// NIC assignment request / response types
// ---------------------------------------------------------------------------

/// Body for `PUT /api/devices/{uuid}/nic-assignments`.
#[derive(Deserialize)]
pub struct PutNicAssignmentRequest {
    /// MAC address of the NIC to configure (must exist on the device).
    pub mac_address: String,
    /// Network to assign this NIC to.  `None` clears both the network and IP.
    pub network_id: Option<i64>,
    /// Static IP within `network_id`'s subnet.  Requires `network_id` to be `Some`.
    pub ip: Option<String>,
}

/// Response body returned by `PUT /api/devices/{uuid}/nic-assignments`.
#[derive(Serialize)]
pub struct NicAssignmentResponse {
    pub mac_address: String,
    pub network_id: Option<i64>,
    pub ip: Option<String>,
}

// ---------------------------------------------------------------------------
// Route registration
// ---------------------------------------------------------------------------

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/devices/{uuid}/label-overrides",
            put(put_label_override),
        )
        .route(
            "/api/devices/{uuid}/label-overrides/{label}",
            delete(delete_label_override),
        )
        .route(
            "/api/devices/{uuid}/nic-assignments",
            put(put_nic_assignment),
        )
        .route(
            "/api/devices/{uuid}/nic-assignments/{mac}",
            delete(delete_nic_assignment),
        )
        .route("/api/devices/{uuid}/warnings", get(get_warnings))
        .route(
            "/api/devices/{uuid}/warnings/{warning_id}",
            delete(delete_warning),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `PUT /api/devices/{uuid}/label-overrides`
///
/// Add or update a single disk label override for the device.  The body must
/// contain `label` (the platform label, e.g. `"ROOT"`) and `path` (the
/// `/dev/disk/by-path/…` value it should resolve to on this device).
///
/// Returns the full updated map of label overrides.
async fn put_label_override(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    Json(req): Json<PutLabelOverrideRequest>,
) -> Result<(StatusCode, Json<LabelOverridesResponse>), HttpError> {
    validate_label_override_request(&req)?;

    let mut conn = state.connection_factory.open().await?;
    let director = Director::new(&mut conn);

    let device = director
        .get_device(&uuid)
        .await
        .map_err(|_| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    let mut attrs = device.attributes.clone();
    attrs.disk_label_overrides.insert(req.label, req.path);
    director.update_attributes_raw(&uuid, &attrs).await?;

    let overrides = build_overrides_response(&attrs.disk_label_overrides);
    Ok((StatusCode::OK, Json(LabelOverridesResponse { overrides })))
}

/// `DELETE /api/devices/{uuid}/label-overrides/{label}`
///
/// Remove a single disk label override by label name.
///
/// Returns `204 No Content` on success, `404` if the device or label is not found.
async fn delete_label_override(
    State(state): State<Arc<AppState>>,
    Path((uuid, label)): Path<(Uuid, String)>,
) -> Result<StatusCode, HttpError> {
    let mut conn = state.connection_factory.open().await?;
    let director = Director::new(&mut conn);

    let device = director
        .get_device(&uuid)
        .await
        .map_err(|_| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    let mut attrs = device.attributes.clone();
    let removed = attrs.disk_label_overrides.remove(&label);
    if removed.is_none() {
        return Err(HttpError::NotFound(format!(
            "Label override '{}' not found on device {}",
            label, uuid
        )));
    }

    director.update_attributes_raw(&uuid, &attrs).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /api/devices/{uuid}/nic-assignments`
///
/// Assign a NIC (identified by MAC address) to a network with an optional static IP.
/// Passing `network_id: null` and `ip: null` clears the assignment.
///
/// Validation:
/// - `mac_address` must match a NIC on the device.
/// - If `network_id` is given, the network must exist.
/// - If `ip` is given, it must be a valid IPv4 address within `network_id`'s subnet.
/// - If `ip` is given without `network_id`, returns 422.
/// - If `ip` is already in use by a different MAC in the same network, returns 422.
async fn put_nic_assignment(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    Json(req): Json<PutNicAssignmentRequest>,
) -> Result<(StatusCode, Json<NicAssignmentResponse>), HttpError> {
    let conn = state.connection_factory.open().await?;

    let mac_lower = validate_mac_and_find_nic(&conn, &uuid, &req.mac_address).await?;
    validate_network_and_ip(&conn, &req, &mac_lower).await?;

    director::store::set_nic_assignment(
        &conn,
        &uuid,
        &req.mac_address,
        req.network_id,
        req.ip.clone(),
    )
    .await?;

    Ok((
        StatusCode::OK,
        Json(NicAssignmentResponse {
            mac_address: req.mac_address,
            network_id: req.network_id,
            ip: req.ip,
        }),
    ))
}

/// Validate the MAC address format on a `PUT` NIC-assignment request and confirm it
/// matches a NIC present on the device's most recent hardware scan.
///
/// Returns the lowercased MAC address on success.
async fn validate_mac_and_find_nic(
    conn: &crate::database::Connection,
    uuid: &Uuid,
    mac_address: &str,
) -> Result<String, HttpError> {
    if !common::is_valid_mac(mac_address) {
        return Err(HttpError::UnprocessableEntity(
            "MAC address must be in the format XX:XX:XX:XX:XX:XX".to_string(),
        ));
    }

    let device = director::store::get_device(conn, uuid)
        .await
        .map_err(|_| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    let mac_lower = mac_address.to_lowercase();
    let nic_exists = device
        .attributes
        .network_interfaces
        .iter()
        .any(|n| n.mac_address.to_lowercase() == mac_lower);

    if !nic_exists {
        return Err(HttpError::NotFound(format!(
            "No NIC with MAC {} found on device {}",
            mac_address, uuid
        )));
    }

    Ok(mac_lower)
}

/// Validate the `network_id`/`ip` portion of a `PUT` NIC-assignment request:
/// - if `network_id` is given, the network must exist;
/// - if `ip` is given, it must be a valid IPv4 address that is a usable host within the
///   network's subnet, and must not already be assigned to a different MAC;
/// - if `ip` is given without `network_id`, the request is rejected.
async fn validate_network_and_ip(
    conn: &crate::database::Connection,
    req: &PutNicAssignmentRequest,
    mac_lower: &str,
) -> Result<(), HttpError> {
    let Some(network_id) = req.network_id else {
        if req.ip.is_some() {
            return Err(HttpError::UnprocessableEntity(
                "An IP address requires a network_id".to_string(),
            ));
        }
        return Ok(());
    };

    let network = dhcp::store::get_network(conn, network_id)
        .await
        .map_err(|_| HttpError::UnprocessableEntity(format!("Network {} not found", network_id)))?;

    let Some(ref ip) = req.ip else {
        return Ok(());
    };

    let subnet: Ipv4Subnet = network
        .subnet
        .parse()
        .map_err(|e| HttpError::UnprocessableEntity(format!("Invalid subnet: {}", e)))?;

    let ip_addr: std::net::Ipv4Addr = ip
        .parse()
        .map_err(|_| HttpError::UnprocessableEntity("Must be a valid IPv4 address".to_string()))?;

    if !subnet.is_assignable_host(ip_addr) {
        return Err(HttpError::UnprocessableEntity(
            "IP address must be a usable host address within the subnet range (network and broadcast addresses are not allowed)".to_string(),
        ));
    }

    // Duplicate-IP check: reject if a different MAC already uses this IP
    if let Some(owner_mac) = director::store::find_mac_using_ip(conn, network_id, ip).await?
        && owner_mac.to_lowercase() != mac_lower
    {
        return Err(HttpError::UnprocessableEntity(format!(
            "IP {} is already assigned to MAC {} in network {}",
            ip, owner_mac, network_id
        )));
    }

    Ok(())
}

/// `DELETE /api/devices/{uuid}/nic-assignments/{mac}`
///
/// Clear the NIC assignment for the given MAC address, removing both `assigned_network_id`
/// and `assigned_ip`.  Returns `204 No Content` on success.
async fn delete_nic_assignment(
    State(state): State<Arc<AppState>>,
    Path((uuid, mac)): Path<(Uuid, String)>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;

    // Verify device exists
    director::store::get_device(&conn, &uuid)
        .await
        .map_err(|_| HttpError::NotFound(format!("Device {} not found", uuid)))?;

    director::store::set_nic_assignment(&conn, &uuid, &mac, None, None)
        .await
        .map_err(|_| {
            HttpError::NotFound(format!("No NIC with MAC {} found on device {}", mac, uuid))
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/devices/{uuid}/warnings`
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

/// `DELETE /api/devices/{uuid}/warnings/{warning_id}`
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate a `PUT` label-override request.
fn validate_label_override_request(req: &PutLabelOverrideRequest) -> Result<(), HttpError> {
    if req.label.trim().is_empty() {
        return Err(HttpError::BadRequest("label must not be empty".to_string()));
    }
    if req.path.trim().is_empty() {
        return Err(HttpError::BadRequest("path must not be empty".to_string()));
    }
    Ok(())
}

/// Convert a `HashMap<String, String>` of overrides into a sorted `Vec` suitable for JSON responses.
fn build_overrides_response(
    overrides: &std::collections::HashMap<String, String>,
) -> Vec<LabelOverrideEntry> {
    let mut entries: Vec<LabelOverrideEntry> = overrides
        .iter()
        .map(|(label, path)| LabelOverrideEntry {
            label: label.clone(),
            path: path.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    entries
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{
        database::{self, DatabaseConnectionFactory},
        test_connection_factory,
    };

    async fn setup_app(
        factory: DatabaseConnectionFactory,
    ) -> (axum::Router, database::Connection, Uuid) {
        let migration_conn = database::run_migrations(&factory).await.unwrap();

        // Create a test device using a known UUID (new_v4 requires the `v4` crate feature)
        let uuid = Uuid::parse_str("d4000000-0000-0000-0000-000000000001").unwrap();
        migration_conn
            .execute(
                "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
                (uuid,),
            )
            .await
            .unwrap();

        let conn_factory: Arc<dyn database::ConnectionFactory> = Arc::new(factory);
        let state = build_test_state(conn_factory);
        let app = routes(state);
        (app, migration_conn, uuid)
    }

    fn build_test_state(conn_factory: Arc<dyn database::ConnectionFactory>) -> Arc<AppState> {
        crate::http::test_helpers::build_test_state(conn_factory)
    }

    #[tokio::test]
    async fn test_put_label_override_adds_entry() {
        let (app, _conn, uuid) = setup_app(test_connection_factory!()).await;

        let body = json!({"label": "ROOT", "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"});
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/label-overrides", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let overrides = json["overrides"].as_array().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0]["label"], "ROOT");
        assert_eq!(
            overrides[0]["path"],
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
    }

    #[tokio::test]
    async fn test_put_label_override_updates_existing_entry() {
        let (app, conn, uuid) = setup_app(test_connection_factory!()).await;

        // Pre-populate an override
        conn.execute(
            "UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.disk_label_overrides', json_object('ROOT', '/dev/disk/by-path/old')) WHERE uuid = ?1",
            (uuid,),
        )
        .await
        .unwrap();

        let body = json!({"label": "ROOT", "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-new"});
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/label-overrides", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let overrides = json["overrides"].as_array().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides[0]["path"],
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1-new"
        );
    }

    #[tokio::test]
    async fn test_put_label_override_empty_label_is_rejected() {
        let (app, _conn, uuid) = setup_app(test_connection_factory!()).await;

        let body = json!({"label": "", "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"});
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/label-overrides", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_put_label_override_device_not_found() {
        let (app, _conn, _uuid) = setup_app(test_connection_factory!()).await;

        let missing_uuid = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000000").unwrap();
        let body = json!({"label": "ROOT", "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"});
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/label-overrides", missing_uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_label_override_removes_entry() {
        let (app, conn, uuid) = setup_app(test_connection_factory!()).await;

        conn.execute(
            "UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.disk_label_overrides', json_object('ROOT', '/dev/disk/by-path/pci-0000:00:1f.2-ata-1')) WHERE uuid = ?1",
            (uuid,),
        )
        .await
        .unwrap();

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/api/devices/{}/label-overrides/ROOT", uuid))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_label_override_not_found() {
        let (app, _conn, uuid) = setup_app(test_connection_factory!()).await;

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/api/devices/{}/label-overrides/NONEXISTENT", uuid))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_warnings_returns_list() {
        let (app, conn, uuid) = setup_app(test_connection_factory!()).await;

        // Insert a device_id lookup and create a warning manually
        let device_id: i64 = conn
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid,), |r| {
                r.get(0)
            })
            .await
            .unwrap();
        conn.execute(
            "INSERT INTO device_warnings (device_id, code, message) VALUES (?1, 'CODE', 'msg')",
            (device_id,),
        )
        .await
        .unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri(format!("/api/devices/{}/warnings", uuid))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let warnings = json.as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["code"], "CODE");
        assert_eq!(warnings[0]["message"], "msg");
    }

    #[tokio::test]
    async fn test_get_warnings_empty() {
        let (app, _conn, uuid) = setup_app(test_connection_factory!()).await;

        let req = Request::builder()
            .method(Method::GET)
            .uri(format!("/api/devices/{}/warnings", uuid))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_delete_warning_dismisses_it() {
        let (app, conn, uuid) = setup_app(test_connection_factory!()).await;

        let device_id: i64 = conn
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid,), |r| {
                r.get(0)
            })
            .await
            .unwrap();
        conn.execute(
            "INSERT INTO device_warnings (device_id, code, message) VALUES (?1, 'CODE', 'msg')",
            (device_id,),
        )
        .await
        .unwrap();
        let warning_id: i64 = conn
            .query_one(
                "SELECT id FROM device_warnings WHERE device_id = ?1",
                (device_id,),
                |r| r.get(0),
            )
            .await
            .unwrap();

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/api/devices/{}/warnings/{}", uuid, warning_id))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_warning_not_found() {
        let (app, _conn, uuid) = setup_app(test_connection_factory!()).await;

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/api/devices/{}/warnings/9999", uuid))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ===== NIC assignment endpoint tests =====

    /// Helper: set up an app with a device that has a NIC and a test network.
    async fn setup_app_with_nic(
        factory: DatabaseConnectionFactory,
    ) -> (axum::Router, database::Connection, Uuid, i64) {
        let migration_conn = database::run_migrations(&factory).await.unwrap();
        let uuid = Uuid::parse_str("d4000000-0000-0000-0000-000000000002").unwrap();

        migration_conn
            .execute(
                "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
                (uuid,),
            )
            .await
            .unwrap();

        // Insert a NIC into device attributes
        migration_conn
            .execute(
                r#"UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.network_interfaces',
                   json('[{"interface_name":"eth0","mac_address":"aa:bb:cc:dd:ee:01","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":null,"assigned_ip":null}]')
                ) WHERE uuid = ?1"#,
                (uuid,),
            )
            .await
            .unwrap();

        // Create a test network
        let network = crate::dhcp::store::create_network(
            &migration_conn,
            "Test Network",
            "10.0.0.0/24",
            Some("10.0.0.1"),
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(
            &migration_conn,
            network.id,
            "Test Pool",
            "10.0.0.100",
            "10.0.0.200",
        )
        .await
        .unwrap();

        let conn_factory: Arc<dyn database::ConnectionFactory> = Arc::new(factory);
        let state = build_test_state(conn_factory);
        let app = routes(state);
        (app, migration_conn, uuid, network.id)
    }

    #[tokio::test]
    async fn test_put_nic_assignment_in_subnet_succeeds() {
        let (app, _conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "10.0.0.50"
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["mac_address"], "aa:bb:cc:dd:ee:01");
        assert_eq!(json["network_id"], network_id);
        assert_eq!(json["ip"], "10.0.0.50");
    }

    #[tokio::test]
    async fn test_put_nic_assignment_network_address_returns_422() {
        let (app, _conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "10.0.0.0"   // network address of 10.0.0.0/24 - not a usable host
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_broadcast_address_returns_422() {
        let (app, _conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "10.0.0.255"   // broadcast address of 10.0.0.0/24 - not a usable host
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_out_of_subnet_returns_422() {
        let (app, _conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "192.168.1.50"   // out of 10.0.0.0/24
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_ip_without_network_returns_422() {
        let (app, _conn, uuid, _network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": null,
            "ip": "10.0.0.50"
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_duplicate_ip_different_mac_returns_422() {
        let (app, conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        // Reserve the IP for a different MAC via static reservation
        crate::dhcp::store::create_static_reservation(
            &conn,
            network_id,
            "ff:ff:ff:ff:ff:ff",
            "10.0.0.50",
            None,
        )
        .await
        .unwrap();

        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "10.0.0.50"
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_same_mac_reconfigures_own_ip_succeeds() {
        let (app, conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        // Set the NIC's own assignment first
        crate::director::store::set_nic_assignment(
            &conn,
            &uuid,
            "aa:bb:cc:dd:ee:01",
            Some(network_id),
            Some("10.0.0.50".to_string()),
        )
        .await
        .unwrap();

        // Re-configuring with the same IP should succeed (same MAC)
        let body = json!({
            "mac_address": "aa:bb:cc:dd:ee:01",
            "network_id": network_id,
            "ip": "10.0.0.50"
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_put_nic_assignment_unknown_mac_returns_404() {
        let (app, _conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let body = json!({
            "mac_address": "ff:ff:ff:ff:ff:ff",
            "network_id": network_id,
            "ip": "10.0.0.50"
        });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/api/devices/{}/nic-assignments", uuid))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_nic_assignment_clears_assignment() {
        let (app, conn, uuid, network_id) = setup_app_with_nic(test_connection_factory!()).await;

        // First assign the NIC
        crate::director::store::set_nic_assignment(
            &conn,
            &uuid,
            "aa:bb:cc:dd:ee:01",
            Some(network_id),
            Some("10.0.0.50".to_string()),
        )
        .await
        .unwrap();

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!(
                "/api/devices/{}/nic-assignments/aa:bb:cc:dd:ee:01",
                uuid
            ))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify it was cleared
        let ifaces = crate::director::store::get_network_interfaces(&conn, &uuid)
            .await
            .unwrap();
        assert_eq!(ifaces[0].assigned_network_id, None);
        assert_eq!(ifaces[0].assigned_ip, None);
    }

    #[tokio::test]
    async fn test_delete_nic_assignment_unknown_mac_returns_404() {
        let (app, _conn, uuid, _network_id) = setup_app_with_nic(test_connection_factory!()).await;

        let req = Request::builder()
            .method(Method::DELETE)
            .uri(format!(
                "/api/devices/{}/nic-assignments/ff:ff:ff:ff:ff:ff",
                uuid
            ))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
