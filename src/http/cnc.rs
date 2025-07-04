use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use serde::Deserialize;

use crate::{database, http::AppState};

#[derive(Deserialize)]
struct IpxeQuery {
    uuid: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .with_state(state)
}

async fn ipxe_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
) -> Result<Response<String>, StatusCode> {
    let uuid = match params.uuid {
        Some(uuid) if !uuid.is_empty() => uuid,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let db = state.db.lock().await;

    let is_known = database::is_device_known(&db, &uuid).map_err(|e| {
        log::error!("Failed to check device: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if !is_known {
        database::register_device(&db, &uuid).map_err(|e| {
            log::error!("Failed to register device: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    drop(db);

    let ipxe_script = if is_known {
        generate_boot_local_script()
    } else {
        generate_intake_script()
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(ipxe_script)
        .unwrap())
}

fn generate_boot_local_script() -> String {
    r#"#!ipxe
# Boot to local disk for known device
sanboot --no-describe --drive 0x80
"#
    .to_string()
}

fn generate_intake_script() -> String {
    r#"#!ipxe
# Boot custom linux image for new device intake
kernel http://rack-director/intake/vmlinuz
initrd http://rack-director/intake/initrd.gz
boot
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use tower::util::ServiceExt;

    async fn setup_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let state = Arc::new(AppState { db: Mutex::new(db) });
        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_ipxe_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state);

        let request = Request::builder()
            .uri("/cnc/ipxe?uuid=550e8400-e29b-41d4-a716-446655440000")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("#!ipxe"));
        assert!(body_str.contains("kernel http://rack-director/intake/vmlinuz"));
    }

    #[tokio::test]
    async fn test_ipxe_known_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440001";

        {
            let db = state.db.lock().await;
            database::register_device(&db, test_uuid).unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .uri(format!("/cnc/ipxe?uuid={test_uuid}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("#!ipxe"));
        assert!(body_str.contains("sanboot --no-describe --drive 0x80"));
    }

    #[tokio::test]
    async fn test_ipxe_missing_uuid() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state);

        let request = Request::builder()
            .uri("/cnc/ipxe")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ipxe_empty_uuid() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state);

        let request = Request::builder()
            .uri("/cnc/ipxe?uuid=")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
