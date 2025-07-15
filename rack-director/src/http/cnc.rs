use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    http::{
        StatusCode,
        header::{self},
    },
    response::Response,
    routing::get,
};
use axum_extra::extract::Host;
use log::warn;
use serde::Deserialize;

use crate::{director::BootTarget, http::AppState};

#[derive(Deserialize)]
struct IpxeQuery {
    uuid: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .with_state(state)
}

// TODO: If uuid is new, register it and return an ipxe menu to discovery.
// TODO: Return valid url to this server
// TODO: Ask director service what a known server should do.
// TODO: Configurable if unknown UUIDs should auto run discovery or not
async fn ipxe_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
    Host(host): Host,
) -> Result<Response<String>, StatusCode> {
    let root_url = format!("http://{host}");

    let uuid = match params.uuid {
        Some(uuid) if !uuid.is_empty() => uuid,
        Some(_) => return Err(StatusCode::BAD_REQUEST),
        None => return Ok(generate_uuid_redirect(&root_url)),
    };

    // Non-fatal, continue anyways.
    if let Err(e) = state.director.register_device(&uuid).await {
        warn!("Couldn't register device {uuid}: {e}");
    };

    let boot_target = match state.director.next_boot_target(&uuid).await {
        Ok(x) => x,
        Err(e) => {
            warn!("Couldn't get boot target from director for {uuid}: {e}");
            return Ok(generate_uuid_redirect(&root_url));
        }
    };

    let ipxe_script = match boot_target {
        BootTarget::LocalDisk => generate_boot_local_script(),
        BootTarget::NetBoot {
            ramdisk,
            kernel,
            cmdline,
        } => generate_kernel_script(&root_url, &ramdisk, &kernel, &cmdline),
    };

    Ok(build_response(ipxe_script))
}

fn generate_boot_local_script() -> String {
    r#"#!ipxe
# Boot to local disk for known device
sanboot --no-describe --drive 0x80
"#
    .to_string()
}

fn generate_kernel_script(root_url: &str, ramdisk: &str, kernel: &str, cmdline: &str) -> String {
    format!(
        r#"#!ipxe
# Boot custom linux image for new device intake
kernel {root_url}/cnc/images/{kernel} {cmdline}
initrd {root_url}/cnc/images/{ramdisk}
boot
"#
    )
}

fn generate_uuid_script(root_url: &str) -> String {
    format!(
        r#"#!ipxe
# Chain boot to send uuid
chain {root_url}/cnc/ipxe?uuid={{uuid}}
"#
    )
}

fn generate_uuid_redirect(root_url: &str) -> Response<String> {
    build_response(generate_uuid_script(root_url))
}

fn build_response(script: String) -> Response<String> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(script)
        .expect("response building should never error")
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
    use tokio::sync::Mutex;
    use tower::util::ServiceExt;

    async fn setup_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let state = Arc::new(AppState {
            director: Director::new(Arc::new(Mutex::new(db))),
        });
        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_ipxe_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state);

        let request = Request::builder()
            .header("Host", "localhost")
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
        assert!(body_str.contains("sanboot --no-describe --drive 0x80"));
    }

    #[tokio::test]
    async fn test_ipxe_known_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440001";

        {
            state.director.register_device(test_uuid).await.unwrap();
        }

        let app = routes(state);

        let request = Request::builder()
            .header("Host", "localhost")
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
            .header("Host", "localhost")
            .uri("/cnc/ipxe")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("#!ipxe"));
        assert!(body_str.contains("chain http://localhost/cnc/ipxe?uuid={uuid}"));
    }

    #[tokio::test]
    async fn test_ipxe_empty_uuid() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state);

        let request = Request::builder()
            .header("Host", "localhost")
            .uri("/cnc/ipxe?uuid=")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
