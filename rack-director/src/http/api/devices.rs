//! `/api/devices` HTTP handlers for device-level disk label overrides and warnings.
//!
//! These endpoints allow operators to pin platform labels to specific disk paths on a
//! per-device basis, and to view or dismiss warnings that the system generates
//! automatically (e.g. when a stale label override is removed).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    device_warnings,
    director::Director,
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

    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

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
    let conn = state.connection_factory.open().await?;
    let director = Director::new(&conn);

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
}
