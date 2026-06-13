use std::sync::Arc;

use axum::{
    Router,
    extract::{self, Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    director::power::PowerState,
    director::{Director, PowerAction},
    http::AppState,
};

use super::devices::ErrorResponse;

/// Response body for `GET /ui/devices/{uuid}/power`.
#[derive(Serialize)]
struct PowerStatusResponse {
    /// Current power state: `"on"`, `"off"`, or `"unknown"`.
    state: PowerState,
    /// Short string identifying the driver in use (`"redfish"`, `"ipmi"`), or
    /// `null` when no BMC is configured.
    driver: Option<String>,
}

/// Request body for `POST /ui/devices/{uuid}/power`.
#[derive(Deserialize)]
struct PowerActionRequest {
    action: PowerAction,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/ui/devices/{uuid}/power",
            get(get_device_power).post(post_device_power),
        )
        .with_state(state)
}

/// `GET /ui/devices/{uuid}/power`
///
/// Query the current power state of a device's BMC.
///
/// Responses:
/// - `200 OK` – Device exists. The body is always `{ "state": "...", "driver": ... }`.
///   When the BMC is not configured, is unreachable, or the driver returns an error, the
///   response **degrades gracefully** to `{ "state": "unknown", "driver": null }` rather
///   than returning an error. This "never errors for a real device" property is relied on
///   by the UI power badge — the badge can always render without hanging or erroring.
/// - `404 Not Found` – No device with that UUID exists in the database.
/// - `500 Internal Server Error` – Database connection could not be opened.
async fn get_device_power(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<PowerStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.connection_factory.open().await.map_err(|e| {
        log::warn!("get_device_power: failed to open DB connection: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    let director = Director::with_power_config(&conn, state.power_config);

    if !director.device_exists(&uuid).await.map_err(|e| {
        log::warn!("get_device_power: device_exists check failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })? {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Device {} not found", uuid),
            }),
        ));
    }

    // Device exists — query power state, degrading gracefully on any BMC failure.
    let (power_state, driver) = director.power_status(&uuid).await;
    Ok(Json(PowerStatusResponse {
        state: power_state,
        driver,
    }))
}

/// `POST /ui/devices/{uuid}/power`
///
/// Issue an OOB power command to a device.
///
/// Request body: `{ "action": "on" | "off" | "cycle" }`.
///
/// `"off"` issues a **hard** (immediate) power-off, not a graceful OS shutdown
/// (see [`Director::power_action`]).
///
/// Responses:
/// - `200 OK`  – command issued successfully.
/// - `404 Not Found` – no device with that UUID exists.
/// - `409 Conflict` – device exists but has no BMC configured.
/// - `500 Internal Server Error` – database connection could not be opened.
/// - `502 Bad Gateway` – BMC driver returned an error.
async fn post_device_power(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
    extract::Json(payload): extract::Json<PowerActionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let conn = state.connection_factory.open().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;
    let director = Director::with_power_config(&conn, state.power_config);

    // Distinguish "device not found" from "no BMC configured" so callers can
    // surface a meaningful error rather than a generic 404 for both cases.
    if !director.device_exists(&uuid).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })? {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Device {} not found", uuid),
            }),
        ));
    }

    match director.power_action(&uuid, payload.action).await {
        Ok(true) => Ok(Json(serde_json::json!({
            "message": format!("Power action issued for device {}", uuid)
        }))),
        Ok(false) => Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("No BMC configured for device {}", uuid),
            }),
        )),
        Err(e) => {
            log::warn!("Power action failed for device {}: {}", uuid, e);
            Err((
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Power command failed: {}", e),
                }),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;
    use uuid::Uuid;

    use crate::{
        database, database::DatabaseConnectionFactory, director::Director, storage::ImageStore,
        test_connection_factory,
    };

    use super::routes;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }

    async fn setup_test_state(
        factory: DatabaseConnectionFactory,
    ) -> (
        Arc<crate::http::AppState>,
        tempfile::TempDir,
        crate::database::Connection,
    ) {
        use tempfile::tempdir;

        let store = ImageStore::new(crate::storage::ImageStoreConfig::Memory {}).unwrap();
        let temp_dir = tempdir().unwrap();

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();
        std::fs::write(agent_images_path.join("vmlinuz"), b"mock kernel data").unwrap();
        std::fs::write(
            agent_images_path.join("initramfs.img"),
            b"mock initramfs data",
        )
        .unwrap();

        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let conn: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        let migration_conn = database::run_migrations(conn.as_ref()).await.unwrap();

        let state = Arc::new(crate::http::AppState {
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

    async fn test_db(state: &crate::http::AppState) -> crate::database::Connection {
        state.connection_factory.open().await.unwrap()
    }

    // ========== Power endpoint tests ==========

    /// `GET /ui/devices/{uuid}/power` on an **existing** device with no BMC must return 200
    /// with `{ "state": "unknown", "driver": null }`.  Proves the endpoint degrades
    /// gracefully for a real device when the BMC is missing.
    #[tokio::test]
    async fn test_get_device_power_no_bmc_returns_200_unknown() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x50);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .method("GET")
            .uri(format!("/ui/devices/{}/power", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "GET /power must always return 200"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["state"], "unknown");
        assert!(json["driver"].is_null());
    }

    /// `POST /ui/devices/{uuid}/power` on an existing device with no BMC must return 409
    /// Conflict (device exists but is not BMC-capable), not 404.
    #[tokio::test]
    async fn test_post_device_power_no_bmc_returns_409() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        let test_uuid = test_uuid(0x51);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .method("POST")
            .uri(format!("/ui/devices/{}/power", test_uuid))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"cycle"}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "POST /power with no BMC must return 409 Conflict"
        );
    }

    /// `GET /ui/devices/{uuid}/power` with a UUID that doesn't exist in the database
    /// must return `404 Not Found`.
    #[tokio::test]
    async fn test_get_device_power_nonexistent_device_returns_404() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        // UUID that was never registered
        let unknown_uuid = test_uuid(0xF1);

        let app = routes(state);

        let request = Request::builder()
            .method("GET")
            .uri(format!("/ui/devices/{}/power", unknown_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "GET /power for an unknown UUID must return 404"
        );
    }

    /// `POST /ui/devices/{uuid}/power` with a UUID that doesn't exist in the database
    /// must return `404 Not Found`.
    #[tokio::test]
    async fn test_post_device_power_nonexistent_device_returns_404() {
        let (state, _temp_dir, _migration_conn) =
            setup_test_state(test_connection_factory!()).await;
        // UUID that was never registered
        let unknown_uuid = test_uuid(0xF2);

        let app = routes(state);

        let request = Request::builder()
            .method("POST")
            .uri(format!("/ui/devices/{}/power", unknown_uuid))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"on"}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /power for an unknown UUID must return 404"
        );
    }
}
