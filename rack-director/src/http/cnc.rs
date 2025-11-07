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

use crate::http::error::Error;

#[derive(Deserialize)]
struct IpxeQuery {
    uuid: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .route("/cnc/install_script", get(install_script_handler))
        .route("/cnc/update_attributes", post(update_attributes))
        .route("/cnc/action_success", post(action_success))
        .route("/cnc/action_failed", post(action_failed))
        .with_state(state)
}

async fn ipxe_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
    Host(host): Host,
) -> Result<Response<String>, Error> {
    let root_url = format!("http://{host}");

    let uuid = match params.uuid {
        Some(uuid) if !uuid.is_empty() => uuid,
        Some(_) => return Err(Error::BadRequest("foo".to_string())),
        None => return Ok(generate_uuid_redirect(&root_url)),
    };

    // Non-fatal, continue anyways.
    if !state.director.device_exists(&uuid).await?
        && let Err(e) = state.director.register_device(&uuid, crate::operating_systems::Architecture::X86_64).await
    {
        warn!("Couldn't register device {uuid}: {e}");
    };

    // Non-fatal. If the boot target can't be found, redirect loop back here to try again
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

async fn install_script_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
) -> Result<Response<String>, Error> {
    let uuid = params
        .uuid
        .ok_or_else(|| Error::BadRequest("Missing uuid parameter".to_string()))?;

    if uuid.is_empty() {
        return Err(Error::BadRequest("Empty uuid parameter".to_string()));
    }

    // Get device
    let device = state
        .director
        .get_device(&uuid)
        .await
        .map_err(|e| Error::InternalServerError(e.to_string()))?;

    // Get device role
    let role = state
        .roles_store
        .get_device_role(&uuid)
        .map_err(|e| Error::InternalServerError(e.to_string()))?
        .ok_or_else(|| Error::NotFound("Device has no role assigned".to_string()))?;

    // Determine device architecture (default to x86-64 for now)
    let arch = crate::operating_systems::Architecture::X86_64;

    // Get OS architecture configuration
    let os_arch = state
        .os_store
        .get_architecture(role.os_id, arch)
        .map_err(|e| Error::NotFound(format!("OS architecture not found: {}", e)))?;

    // Get OS
    let os = state
        .os_store
        .get(role.os_id)
        .map_err(|e| Error::InternalServerError(e.to_string()))?;

    // Check if install script exists
    let script_path = os_arch
        .install_script_path
        .ok_or_else(|| Error::NotFound("No install script for this OS architecture".to_string()))?;

    // Download install script template from storage
    let script_bytes = state
        .image_store
        .download(&script_path)
        .await
        .map_err(|e| Error::InternalServerError(format!("Failed to download script: {}", e)))?;

    let template = String::from_utf8(script_bytes)
        .map_err(|e| Error::InternalServerError(format!("Script is not valid UTF-8: {}", e)))?;

    // Get device network info
    let network_info = get_device_network_info(&state, &uuid).await?;

    // Get device attributes
    let attributes = device.get("attributes")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let hostname = attributes
        .get("hostname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let device_info = crate::templates::DeviceInfo {
        uuid: uuid.clone(),
        hostname,
    };

    // Render template with device context
    let rendered = crate::templates::render_install_script(&template, &device_info, &role, &os, &network_info)
        .map_err(|e| Error::InternalServerError(format!("Template rendering failed: {}", e)))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
}

async fn get_device_network_info(
    state: &Arc<AppState>,
    uuid: &str,
) -> Result<crate::templates::NetworkInfo, Error> {
    // Try to find device's lease
    let lease = state
        .dhcp_store
        .find_lease_by_device_uuid(uuid)
        .map_err(|e| Error::InternalServerError(e.to_string()))?;

    if let Some(lease) = lease {
        // Get DHCP config for gateway and DNS
        let config = state
            .dhcp_store
            .get_config()
            .map_err(|e| Error::InternalServerError(e.to_string()))?;

        let dns_servers: Vec<String> = serde_json::from_str(&config.dns_servers)
            .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

        Ok(crate::templates::NetworkInfo {
            mac_address: lease.mac_address,
            ip_address: lease.ip_address,
            gateway: config.gateway,
            dns_servers,
            netmask: "255.255.255.0".to_string(), // TODO: Calculate from subnet
        })
    } else {
        Err(Error::NotFound(
            "Device has no DHCP lease".to_string(),
        ))
    }
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
        let db = Arc::new(Mutex::new(db));

        // Create image store for testing
        let storage_path = temp_dir.path().join("images");
        let image_store = crate::storage::LocalImageStore::new(
            storage_path,
            "http://localhost:8080/images".to_string(),
        ).unwrap();

        let state = Arc::new(AppState {
            director: Director::new(db.clone()),
            dhcp_store: crate::dhcp::DhcpStore::new(db.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db.clone()),
            roles_store: crate::roles::RolesStore::new(db),
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
            state.director.register_device(test_uuid, crate::operating_systems::Architecture::X86_64).await.unwrap();
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
        state.director.register_device(test_uuid, crate::operating_systems::Architecture::X86_64).await.unwrap();
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
        state.director.register_device(test_uuid, crate::operating_systems::Architecture::X86_64).await.unwrap();
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
        state.director.register_device(test_uuid, crate::operating_systems::Architecture::X86_64).await.unwrap();

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
