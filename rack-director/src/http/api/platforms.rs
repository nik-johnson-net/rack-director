//! `/api/platforms` HTTP handlers for platform disk label editing.
//!
//! These endpoints allow operators to rename or clear a disk label on a platform,
//! which controls how provisioning templates resolve labels (e.g. ROOT, DATA1) to
//! device paths.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::put,
};
use serde::Deserialize;

use crate::{
    http::{AppState, error::Error as HttpError},
    platforms::Platform,
};

// ---------------------------------------------------------------------------
// Request type
// ---------------------------------------------------------------------------

/// Body for `PUT /api/platforms/{id}/disks/{index}/label`.
///
/// Send `{"label": "CACHE"}` to rename, or `{"label": null}` to clear.
#[derive(Deserialize)]
pub struct UpdateDiskLabelRequest {
    /// The new label value, or `null` to clear the label entirely.
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Route registration
// ---------------------------------------------------------------------------

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/platforms/{id}/disks/{index}/label",
            put(update_disk_label),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `PUT /api/platforms/{id}/disks/{index}/label`
///
/// Rename or clear the label on a single disk entry within a platform.
///
/// - `{id}` — integer platform ID
/// - `{index}` — zero-based index into the platform's disk list
/// - Body: `{"label": "DATA2"}` or `{"label": null}` to clear
///
/// Returns the full updated platform on success.
/// Returns 404 if the platform is not found or the disk index is out of bounds.
/// Returns 422 if the label already exists on a different disk in the same platform.
async fn update_disk_label(
    State(state): State<Arc<AppState>>,
    Path((id, index)): Path<(i64, usize)>,
    Json(req): Json<UpdateDiskLabelRequest>,
) -> Result<(StatusCode, Json<Platform>), HttpError> {
    let conn = state.connection_factory.open().await?;

    let platform =
        crate::platforms::store::update_disk_label(&conn, id, index, req.label.as_deref())
            .await
            .map_err(|e| map_update_disk_label_error(e, id, index))?;

    Ok((StatusCode::OK, Json(platform)))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map store errors from `update_disk_label` to the appropriate HTTP error.
fn map_update_disk_label_error(e: anyhow::Error, id: i64, index: usize) -> HttpError {
    let msg = e.to_string();
    if msg.contains("Platform not found") {
        HttpError::NotFound(format!("Platform {id} not found"))
    } else if msg.contains("out of bounds") {
        HttpError::NotFound(format!(
            "Disk index {index} is out of bounds for platform {id}"
        ))
    } else if msg.contains("Label already exists") {
        HttpError::UnprocessableEntity(msg)
    } else {
        HttpError::ServerInternalError(e)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::{
        database::{self, DatabaseConnectionFactory},
        platforms::{DiskType, PlatformAttributes, PlatformCpu, PlatformDisk, PlatformNic},
        test_connection_factory,
    };

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    async fn setup_app(factory: DatabaseConnectionFactory) -> (axum::Router, database::Connection) {
        let migration_conn = database::run_migrations(&factory).await.unwrap();

        let conn_factory: Arc<dyn database::ConnectionFactory> = Arc::new(factory);
        let state = build_test_state(conn_factory);
        let app = routes(state);
        (app, migration_conn)
    }

    /// Build a minimal `AppState` for handler tests.
    fn build_test_state(conn_factory: Arc<dyn database::ConnectionFactory>) -> Arc<AppState> {
        let temp_dir = tempfile::tempdir().unwrap();
        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();
        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());
        let image_store =
            crate::storage::ImageStore::new(crate::storage::ImageStoreConfig::Memory {
                base_url: "http://localhost/images".into(),
            })
            .unwrap();
        // Leak TempDir so paths remain valid for the test duration.
        std::mem::forget(temp_dir);
        Arc::new(AppState {
            connection_factory: conn_factory,
            image_store: image_store.into(),
            agent_images_path,
            boot_file_provider,
            dhcp: crate::dhcp::DhcpControl::noop(),
            unprovisioned_sleep_secs: 0,
        })
    }

    fn sample_attributes() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![PlatformNic {
                logical: "eno1".to_string(),
                speed_mbps: Some(1000),
                label: Some("NIC1".to_string()),
            }],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "Xeon".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        }
    }

    async fn create_platform(conn: &database::Connection) -> i64 {
        crate::platforms::store::create(conn, "Test Platform", None, &sample_attributes())
            .await
            .unwrap()
            .id
            .unwrap()
    }

    fn put_label_request(
        platform_id: i64,
        index: usize,
        label: serde_json::Value,
    ) -> Request<Body> {
        let body = json!({ "label": label });
        Request::builder()
            .method(Method::PUT)
            .uri(format!(
                "/api/platforms/{}/disks/{}/label",
                platform_id, index
            ))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_update_label_renames_and_returns_platform() {
        let (app, conn) = setup_app(test_connection_factory!()).await;
        let platform_id = create_platform(&conn).await;

        let req = put_label_request(platform_id, 1, json!("CACHE"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let platform: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(platform["attributes"]["disks"][1]["label"], "CACHE");
        // Other disks unchanged
        assert_eq!(platform["attributes"]["disks"][0]["label"], "ROOT");
    }

    #[tokio::test]
    async fn test_update_label_clears_label() {
        let (app, conn) = setup_app(test_connection_factory!()).await;
        let platform_id = create_platform(&conn).await;

        let req = put_label_request(platform_id, 0, serde_json::Value::Null);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let platform: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // After clearing, the label key should be null or absent
        assert!(
            platform["attributes"]["disks"][0]["label"].is_null(),
            "expected null label, got {:?}",
            platform["attributes"]["disks"][0]["label"]
        );
    }

    #[tokio::test]
    async fn test_update_label_same_label_on_same_disk_is_allowed() {
        let (app, conn) = setup_app(test_connection_factory!()).await;
        let platform_id = create_platform(&conn).await;

        let req = put_label_request(platform_id, 0, json!("ROOT"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_update_label_duplicate_returns_422() {
        let (app, conn) = setup_app(test_connection_factory!()).await;
        let platform_id = create_platform(&conn).await;

        // "ROOT" already belongs to disk 0; setting it on disk 1 must return 422
        let req = put_label_request(platform_id, 1, json!("ROOT"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_update_label_index_out_of_bounds_returns_404() {
        let (app, conn) = setup_app(test_connection_factory!()).await;
        let platform_id = create_platform(&conn).await;

        let req = put_label_request(platform_id, 99, json!("X"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_label_platform_not_found_returns_404() {
        let (app, _conn) = setup_app(test_connection_factory!()).await;

        let req = put_label_request(9999, 0, json!("ROOT"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
