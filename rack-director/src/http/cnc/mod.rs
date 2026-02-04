mod boot_files;
mod device_registration;
mod install_script;
mod ipxe_scripts;
mod network_processing;

use std::sync::Arc;

use axum::{
    Router,
    extract::{self, ConnectInfo, Query, State},
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
use std::net::SocketAddr;
use uuid::Uuid;

use crate::{director::BootTarget, director::NetworkInterface, http::AppState};

use crate::http::error::Error;

use ipxe_scripts::{
    build_response, generate_boot_local_script, generate_kernel_script, generate_uuid_redirect,
};

#[derive(Debug, Deserialize)]
struct IpxeQuery {
    uuid: Option<Uuid>,
    mac: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .route("/cnc/install_script", get(install_script_handler))
        .route("/cnc/agent-images/{filename}", get(agent_images_handler))
        .route("/cnc/boot/{filename}", get(boot_files::boot_file_handler))
        .route("/cnc/update_attributes", post(update_attributes))
        .route("/cnc/action_success", post(action_success))
        .route("/cnc/action_failed", post(action_failed))
        .route("/cnc/devices/{uuid}/bmc_config", get(get_bmc_config))
        .with_state(state)
}

async fn ipxe_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<IpxeQuery>,
    Host(host): Host,
) -> Result<Response<String>, Error> {
    log::debug!("/cnc/ipxe, params: {:?}", params);
    let root_url = format!("http://{host}");

    let uuid: Uuid = match params.uuid {
        Some(uuid) => uuid,
        None => return Ok(generate_uuid_redirect(&root_url)),
    };

    // Resolve MAC address from parameter or DHCP lookup
    let mac_address =
        device_registration::resolve_mac_address(&state, params.mac.as_ref(), addr).await;

    // Register device if it doesn't exist and automatically start discovery
    if !state.director.device_exists(&uuid).await? {
        device_registration::register_and_start_discovery(&state, &uuid, mac_address.as_ref())
            .await;
    }

    // Store network info from DHCP lease (reuse the MAC address lookup from above)
    if let Some(mac) = &mac_address {
        if let Ok(Some(lease)) = state.dhcp_store.get_lease_by_mac(mac).await {
            log::info!(
                "Found DHCP lease for device {}: MAC {} IP {}",
                uuid,
                lease.mac_address,
                lease.ip_address
            );

            // Update IP address for the interface with this MAC
            // This will also create the interface if it doesn't exist yet
            if let Err(e) = state
                .director
                .set_device_ip_address(&uuid, &lease.ip_address, &lease.mac_address)
                .await
            {
                warn!("Couldn't store IP address for device {uuid}: {e}");
            }

            // Update legacy MAC address field for backward compatibility
            if let Err(e) = state
                .director
                .set_device_mac_address(&uuid, &lease.mac_address)
                .await
            {
                warn!("Couldn't store MAC address for device {uuid}: {e}");
            }
        }
    } else {
        log::debug!("No MAC address available for device {}", uuid);
    }

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
        } => generate_kernel_script(&ramdisk, &kernel, &cmdline),
    };

    log::debug!("cnc/ipxe: returning script for {}:\n{}", uuid, ipxe_script);

    Ok(build_response(ipxe_script))
}

async fn install_script_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
) -> Result<Response<String>, Error> {
    let uuid = params
        .uuid
        .ok_or_else(|| Error::BadRequest("Missing uuid parameter".to_string()))?;

    install_script::render_for_device(&state, &uuid).await
}

async fn agent_images_handler(
    State(state): State<Arc<AppState>>,
    extract::Path(filename): extract::Path<String>,
) -> Result<(StatusCode, [(header::HeaderName, &'static str); 1], Vec<u8>), Error> {
    // Construct the full file path
    let file_path = state.agent_images_path.join(&filename);

    // Canonicalize both paths to prevent directory traversal attacks
    // This resolves all symlinks, .., ., etc.
    let canonical_file = tokio::fs::canonicalize(&file_path).await.map_err(|e| {
        warn!("Failed to canonicalize path for {}: {}", filename, e);
        Error::NotFound(format!("Agent image not found: {}", filename))
    })?;

    let canonical_base = tokio::fs::canonicalize(&state.agent_images_path)
        .await
        .map_err(|e| {
            warn!("Failed to canonicalize base path: {}", e);
            Error::NotFound(format!("Agent image not found: {}", filename))
        })?;

    // Verify the resolved file path is within the base directory
    // Return NotFound for security violations to avoid leaking information
    if !canonical_file.starts_with(&canonical_base) {
        warn!(
            "Directory traversal attempt blocked: {} resolves outside base directory",
            filename
        );
        return Err(Error::NotFound(format!(
            "Agent image not found: {}",
            filename
        )));
    }

    // Read and serve the file
    let data = tokio::fs::read(&canonical_file).await.map_err(|e| {
        warn!("Failed to read agent image {}: {}", filename, e);
        Error::NotFound(format!("Agent image not found: {}", filename))
    })?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        data,
    ))
}

#[derive(Deserialize, Serialize)]
struct UpdateAttributesQuery {
    uuid: Uuid,
    attributes: serde_json::Map<String, serde_json::Value>,
}

#[axum::debug_handler]
async fn update_attributes(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<UpdateAttributesQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;
    let attributes = payload.attributes;

    // First, store the attributes as provided by the agent
    if let Err(e) = state
        .director
        .update_attributes(&uuid, attributes.clone())
        .await
    {
        warn!("Couldn't update attributes for {uuid}: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Then, if network_interfaces were provided, backfill IP addresses from DHCP leases
    if let Some(network_interfaces_value) = attributes.get("network_interfaces")
        && let Some(interfaces_array) = network_interfaces_value.as_array()
    {
        // Parse interfaces from JSON
        let interfaces: Vec<NetworkInterface> = interfaces_array
            .iter()
            .filter_map(
                |value| match serde_json::from_value::<NetworkInterface>(value.clone()) {
                    Ok(nic) => Some(nic),
                    Err(e) => {
                        warn!("Failed to parse network interface from JSON: {}", e);
                        None
                    }
                },
            )
            .collect();

        // Enrich interfaces with DHCP lease information
        let mut enriched_interfaces =
            network_processing::enrich_interfaces_with_dhcp_info(&state, interfaces).await;

        // Detect and mark duplicate MACs on the same network
        network_processing::detect_and_mark_duplicates(&state, &uuid, &mut enriched_interfaces)
            .await;

        // Update the device with IP-enriched network interfaces
        if !enriched_interfaces.is_empty()
            && let Err(e) = state
                .director
                .set_network_interfaces(&uuid, &enriched_interfaces)
                .await
        {
            warn!(
                "Couldn't set enriched network interfaces for {}: {}",
                uuid, e
            );
        }

        // Complete any pending devices whose MACs match the device's interfaces
        network_processing::complete_pending_devices_for_interfaces(
            &state,
            &uuid,
            &enriched_interfaces,
        )
        .await;
    }

    Ok(NoContent)
}

#[derive(Deserialize, Serialize)]
struct ActionStatusQuery {
    uuid: Uuid,
}

#[derive(Deserialize, Serialize)]
struct ActionFailedQuery {
    uuid: Uuid,
    error_message: String,
}

fn default_ip_source() -> String {
    "static".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcConfig {
    #[serde(default = "default_ip_source")]
    pub ip_address_source: String, // "static" or "dhcp"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netmask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
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

/// Get BMC configuration for a device
///
/// This endpoint returns the BMC configuration stored in the device's attributes.
/// The configuration includes static IP settings and credentials that will be
/// applied to the BMC by the rack-agent.
#[axum::debug_handler]
async fn get_bmc_config(
    State(state): State<Arc<AppState>>,
    extract::Path(uuid): extract::Path<Uuid>,
) -> Result<extract::Json<BmcConfig>, StatusCode> {
    // Get device
    let device = match state.director.get_device(&uuid).await {
        Ok(device) => device,
        Err(e) => {
            warn!("Failed to get device {}: {}", uuid, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // Extract BMC config from device attributes
    let bmc_config_value = device.attributes.get("bmc_config").ok_or_else(|| {
        warn!("Device {} has no BMC configuration", uuid);
        StatusCode::NOT_FOUND
    })?;

    // Deserialize BMC config
    let bmc_config: BmcConfig = serde_json::from_value(bmc_config_value.clone()).map_err(|e| {
        warn!("Failed to parse BMC config for device {}: {}", uuid, e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Validate configuration based on ip_address_source
    if bmc_config.ip_address_source == "static" {
        // Ensure required fields are present for static configuration
        if bmc_config.ip_address.is_none()
            || bmc_config.netmask.is_none()
            || bmc_config.gateway.is_none()
        {
            warn!(
                "Static BMC config missing required fields for device {}",
                uuid
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    Ok(extract::Json(bmc_config))
}

#[cfg(test)]
mod tests {
    use crate::{database, director::Director, storage::MemoryImageStore};

    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }

    async fn setup_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let db_tokio = Arc::new(tokio::sync::Mutex::new(db));

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

        // Create mock agent image files
        std::fs::write(agent_images_path.join("vmlinuz"), b"mock kernel data").unwrap();
        std::fs::write(
            agent_images_path.join("initramfs.img"),
            b"mock initramfs data",
        )
        .unwrap();

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        // Create mock boot files
        std::fs::write(boot_files_path.join("ipxe.efi"), b"mock ipxe.efi").unwrap();
        std::fs::write(boot_files_path.join("undionly.kpxe"), b"mock undionly.kpxe").unwrap();

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
    async fn test_ipxe_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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
        // New devices now automatically start discovery, so they boot the agent image
        assert!(body_str.contains("kernel"));
        assert!(body_str.contains("/cnc/agent-images/vmlinuz"));
        assert!(body_str.contains("/cnc/agent-images/initramfs.img"));
    }

    #[tokio::test]
    async fn test_ipxe_known_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let uuid = test_uuid(1);

        {
            state
                .director
                .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let request = Request::builder()
            .header("Host", "localhost")
            .uri(format!("/cnc/ipxe?uuid={}", uuid))
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
        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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
        assert!(body_str.contains("chain http://localhost/cnc/ipxe?uuid=${uuid}&mac=${netX/mac}"));
    }

    #[tokio::test]
    async fn test_ipxe_empty_uuid() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let request = Request::builder()
            .header("Host", "localhost")
            .uri("/cnc/ipxe?uuid=")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ipxe_handler_with_mac_parameter() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x10);
        let test_mac = "aa:bb:cc:dd:ee:ff";

        // Create a pending device for this MAC
        state
            .director
            .create_pending_device(test_mac, 1)
            .await
            .unwrap();

        // Verify pending device exists
        let pending_id = state
            .director
            .find_pending_device_by_mac(test_mac)
            .await
            .unwrap();
        assert!(pending_id.is_some());

        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        // Make request with MAC parameter
        let request = Request::builder()
            .header("Host", "localhost")
            .uri(format!("/cnc/ipxe?uuid={}&mac={}", test_uuid, test_mac))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Device should be registered
        assert!(state.director.device_exists(&test_uuid).await.unwrap());

        // Pending device should be completed (removed from pending_devices table)
        let pending_id = state
            .director
            .find_pending_device_by_mac(test_mac)
            .await
            .unwrap();
        assert!(pending_id.is_none(), "Pending device should be completed");
    }

    #[tokio::test]
    async fn test_action_success() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x03);

        // Create a test plan
        let actions = vec![crate::plans::Action::InstallOs];
        let plan = crate::plans::Plan::new(test_uuid, actions);

        // Register device and create plan
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionStatusQuery { uuid: test_uuid };

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
        let test_uuid = test_uuid(0x04);

        // Create a test plan
        let actions = vec![crate::plans::Action::InstallOs];
        let plan = crate::plans::Plan::new(test_uuid, actions);

        // Register device and create plan
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionFailedQuery {
            uuid: test_uuid,
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
        let test_uuid = test_uuid(0x05);

        // Register device but don't create a plan
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionStatusQuery { uuid: test_uuid };

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_automatic_discovery_on_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x99);

        // Verify device doesn't exist yet
        assert!(!state.director.device_exists(&test_uuid).await.unwrap());

        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        // First boot - device registers and discovery starts
        let request = Request::builder()
            .header("Host", "localhost")
            .uri(format!("/cnc/ipxe?uuid={}", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Device should now exist
        assert!(state.director.device_exists(&test_uuid).await.unwrap());

        // Device should be in "new" state
        let lifecycle = state
            .director
            .get_device_lifecycle(&test_uuid)
            .await
            .unwrap();
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Device should have an active discovery plan with 2 actions
        let active_plan = state
            .director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0], crate::plans::Action::DiscoverHardware);
        assert_eq!(plan.actions[1], crate::plans::Action::ConfigureBmc);

        // iPXE response should contain agent kernel boot
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("#!ipxe"));
        assert!(body_str.contains("kernel"));
        assert!(body_str.contains("/cnc/agent-images/vmlinuz"));
        assert!(body_str.contains("/cnc/agent-images/initramfs.img"));
        assert!(body_str.contains("rackdirector.url="));
    }

    #[tokio::test]
    async fn test_discovery_completion_flow() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x98);

        // Register device and start discovery transition
        state
            .director
            .register_device(&test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        state
            .director
            .start_lifecycle_transition(
                &test_uuid,
                crate::lifecycle::DeviceLifecycle::Unprovisioned,
            )
            .await
            .unwrap();

        // Simulate agent updating attributes
        let update_payload = UpdateAttributesQuery {
            uuid: test_uuid,
            attributes: {
                let mut attrs = serde_json::Map::new();
                attrs.insert(
                    "manufacturer".to_string(),
                    serde_json::Value::String("Dell Inc.".to_string()),
                );
                attrs.insert(
                    "product_name".to_string(),
                    serde_json::Value::String("PowerEdge R640".to_string()),
                );
                attrs
            },
        };

        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/update_attributes")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&update_payload).unwrap()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify attributes were updated
        let device = state.director.get_device(&test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("manufacturer").unwrap().as_str(),
            Some("Dell Inc.")
        );

        // Simulate agent reporting success for first action (discover_hardware)
        let success_payload = ActionStatusQuery { uuid: test_uuid };

        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&success_payload).unwrap()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify device is still in New state (configure_bmc action still pending)
        let lifecycle = state
            .director
            .get_device_lifecycle(&test_uuid)
            .await
            .unwrap();
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Simulate agent reporting success for second action (configure_bmc)
        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&success_payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify device transitioned to Unprovisioned after both actions complete
        let lifecycle = state
            .director
            .get_device_lifecycle(&test_uuid)
            .await
            .unwrap();
        assert_eq!(
            lifecycle,
            Some(crate::lifecycle::DeviceLifecycle::Unprovisioned)
        );

        // Verify no active plan
        let active_plan = state
            .director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_none());
    }

    #[tokio::test]
    async fn test_agent_images_endpoint() {
        let (state, _temp_dir) = setup_test_state().await;
        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        // Test fetching vmlinuz
        let request = Request::builder()
            .uri("/cnc/agent-images/vmlinuz")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"mock kernel data");

        // Test fetching initramfs.img
        let request = Request::builder()
            .uri("/cnc/agent-images/initramfs.img")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"mock initramfs data");

        // Test directory traversal protection - returns 404 to avoid leaking info
        let request = Request::builder()
            .uri("/cnc/agent-images/..%2F..%2Fetc%2Fpasswd")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test non-existent file
        let request = Request::builder()
            .uri("/cnc/agent-images/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
