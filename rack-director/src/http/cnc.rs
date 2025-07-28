use std::sync::Arc;

use axum::{
    Router,
    extract::{self, Query, State},
    http::{
        StatusCode,
        header::{self},
    },
    response::{NoContent, Response},
    routing::{get, post},
};
use axum_extra::extract::Host;
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{director::BootTarget, http::AppState};

#[derive(Deserialize)]
struct IpxeQuery {
    uuid: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .route("/cnc/update_attributes", post(update_attributes))
        .route("/cnc/action_success", post(action_success))
        .route("/cnc/action_failed", post(action_failed))
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

#[derive(Deserialize)]
struct UpdateAttributesQuery {
    uuid: String,
    attributes: serde_json::Map<String, serde_json::Value>,
}

#[axum::debug_handler]
async fn update_attributes(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<UpdateAttributesQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;
    let attributes = payload.attributes;
    if let Err(e) = state.director.update_attributes(&uuid, attributes).await {
        warn!("Couldn't update attributes for {uuid}: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok(NoContent)
}

#[derive(Deserialize, Serialize)]
struct ActionStatusQuery {
    uuid: String,
}

#[derive(Deserialize, Serialize)]
struct ActionFailedQuery {
    uuid: String,
    error_message: String,
}

#[axum::debug_handler]
async fn action_success(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<ActionStatusQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;

    match state.director.mark_action_success(&uuid).await {
        Ok(_) => Ok(NoContent),
        Err(e) => {
            warn!("Couldn't mark action success for {uuid}: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[axum::debug_handler]
async fn action_failed(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<ActionFailedQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;
    let error_message = payload.error_message;

    match state
        .director
        .mark_action_failed(&uuid, &error_message)
        .await
    {
        Ok(_) => Ok(NoContent),
        Err(e) => {
            warn!("Couldn't mark action failed for {uuid}: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
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

    #[tokio::test]
    async fn test_action_success() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440003";

        // Create a test plan
        let actions = vec![
            crate::plans::Action::new("install_os".to_string(), std::collections::HashMap::new()),
            crate::plans::Action::new(
                "configure_network".to_string(),
                std::collections::HashMap::new(),
            ),
        ];
        let plan = crate::plans::Plan::new(test_uuid.to_string(), actions);

        // Register device and create plan
        state.director.register_device(test_uuid).await.unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state);

        let payload = ActionStatusQuery {
            uuid: test_uuid.to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_action_failed() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440004";

        // Create a test plan
        let actions = vec![crate::plans::Action::new(
            "install_os".to_string(),
            std::collections::HashMap::new(),
        )];
        let plan = crate::plans::Plan::new(test_uuid.to_string(), actions);

        // Register device and create plan
        state.director.register_device(test_uuid).await.unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state);

        let payload = ActionFailedQuery {
            uuid: test_uuid.to_string(),
            error_message: "Installation failed".to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_failed")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_action_success_no_plan() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440005";

        // Register device but don't create a plan
        state.director.register_device(test_uuid).await.unwrap();

        let app = routes(state);

        let payload = ActionStatusQuery {
            uuid: test_uuid.to_string(),
        };

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
