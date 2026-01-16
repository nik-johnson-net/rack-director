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

use crate::{director::BootTarget, director::NetworkInterface, http::AppState};

use crate::http::error::Error;

#[derive(Deserialize)]
struct IpxeQuery {
    uuid: Option<String>,
    mac: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/cnc/ipxe", get(ipxe_handler))
        .route("/cnc/install_script", get(install_script_handler))
        .route("/cnc/agent-images/{filename}", get(agent_images_handler))
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
    let root_url = format!("http://{host}");

    let uuid = match params.uuid {
        Some(uuid) if !uuid.is_empty() => uuid,
        Some(_) => return Err(Error::BadRequest("UUID cannot be empty".to_string())),
        None => return Ok(generate_uuid_redirect(&root_url)),
    };

    // Prefer MAC from query parameter, fall back to IP-based lookup for backward compatibility
    let mac_address = match &params.mac {
        Some(mac) if !mac.is_empty() => Some(mac.clone()),
        _ => {
            // Fallback: Look up MAC address from client IP (may not work in all network setups)
            let client_ip = addr.ip().to_string();
            if let Ok(leases) = state.dhcp_store.get_all_leases().await {
                leases
                    .iter()
                    .find(|l| l.ip_address == client_ip)
                    .map(|l| l.mac_address.clone())
            } else {
                None
            }
        }
    };

    // Register device if it doesn't exist and automatically start discovery
    if !state.director.device_exists(&uuid).await? {
        // Check for pending device
        if let Some(mac) = &mac_address
            && let Ok(Some(_)) = state.director.find_pending_device_by_mac(mac).await
        {
            log::info!(
                "Completing pending device for MAC {} with UUID {}",
                mac,
                uuid
            );
        }

        // Register device (existing code)
        if let Err(e) = state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
        {
            warn!("Couldn't register device {uuid}: {e}");
        } else {
            // Complete pending device link (NEW)
            if let Some(mac) = &mac_address
                && let Err(e) = state.director.complete_pending_device(mac, &uuid).await
            {
                warn!("Couldn't complete pending device: {}", e);
            }

            // Automatically start discovery transition for newly registered devices
            if let Err(e) = state
                .director
                .start_lifecycle_transition(&uuid, crate::lifecycle::DeviceLifecycle::Unprovisioned)
                .await
            {
                warn!("Couldn't start discovery transition for {uuid}: {e}");
            }
        }
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
        .map_err(Error::ServerInternalError)?;

    // Get device role
    let role = state
        .roles_store
        .get_device_role(&uuid)
        .await
        .map_err(Error::ServerInternalError)?
        .ok_or_else(|| Error::NotFound("Device has no role assigned".to_string()))?;

    // Get device architecture
    let arch = device.architecture;

    // Get OS architecture configuration
    let os_arch = state
        .os_store
        .get_architecture(role.os_id, arch)
        .await
        .map_err(|e| Error::NotFound(format!("OS architecture not found: {}", e)))?;

    // Get OS
    let os = state
        .os_store
        .get(role.os_id)
        .await
        .map_err(Error::ServerInternalError)?;

    // Check if install script exists
    let script_path = os_arch
        .install_script_path
        .ok_or_else(|| Error::NotFound("No install script for this OS architecture".to_string()))?;

    // Download install script template from storage
    let script_bytes = state
        .image_store
        .download(&script_path)
        .await
        .map_err(|e| {
            Error::ServerInternalError(anyhow::anyhow!("Failed to download script: {}", e))
        })?;

    let template = String::from_utf8(script_bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Script is not valid UTF-8: {}", e))
    })?;

    // Get device network info
    let network_info = get_device_network_info(&state, &uuid).await?;

    // Get device attributes
    let hostname = device
        .attributes
        .get("hostname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let device_info = crate::templates::DeviceInfo {
        uuid: uuid.clone(),
        hostname,
    };

    // Render template with device context
    let rendered =
        crate::templates::render_install_script(&template, &device_info, &role, &os, &network_info)
            .map_err(|e| {
                Error::ServerInternalError(anyhow::anyhow!("Template rendering failed: {}", e))
            })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
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

async fn get_device_network_info(
    state: &Arc<AppState>,
    uuid: &str,
) -> Result<crate::templates::NetworkInfo, Error> {
    // Try to find device's lease
    let lease = state
        .dhcp_store
        .find_lease_by_device_uuid(uuid)
        .map_err(Error::ServerInternalError)?;

    if let Some(lease) = lease {
        // Get DHCP config for gateway and DNS
        let network = state.dhcp_store.get_network(lease.id).await?;
        let dns_servers = network.dns_servers;

        Ok(crate::templates::NetworkInfo {
            mac_address: lease.mac_address,
            ip_address: lease.ip_address,
            gateway: network.gateway,
            dns_servers,
            netmask: network.subnet, // TODO: Calculate from subnet
        })
    } else {
        Err(Error::NotFound("Device has no DHCP lease".to_string()))
    }
}

fn generate_uuid_script(root_url: &str) -> String {
    format!(
        r#"#!ipxe
# Chain boot to send uuid and mac
chain {root_url}/cnc/ipxe?uuid={{uuid}}&mac={{netX/mac}}
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

#[derive(Deserialize, Serialize)]
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
        let mut enriched_interfaces: Vec<NetworkInterface> = Vec::new();

        for interface_value in interfaces_array {
            // Parse the interface from JSON
            match serde_json::from_value::<NetworkInterface>(interface_value.clone()) {
                Ok(mut nic) => {
                    // Look up DHCP lease for this MAC
                    if let Ok(Some(lease)) =
                        state.dhcp_store.get_lease_by_mac(&nic.mac_address).await
                    {
                        log::info!(
                            "Backfilling IP {} and network_id {} for NIC {} (MAC {})",
                            lease.ip_address,
                            lease.network_id.unwrap_or(-1),
                            nic.interface_name,
                            nic.mac_address
                        );
                        nic.ip_address = Some(lease.ip_address);
                        nic.network_id = lease.network_id;
                    }
                    enriched_interfaces.push(nic);
                }
                Err(e) => {
                    warn!("Failed to parse network interface from JSON: {}", e);
                }
            }
        }

        // Detect duplicate MACs on the same network
        for nic in &mut enriched_interfaces {
            // Only check for duplicates if the interface has a network_id
            if let Some(network_id) = nic.network_id {
                match state
                    .director
                    .find_duplicate_macs_on_network(&nic.mac_address, network_id, &uuid)
                    .await
                {
                    Ok(duplicates) if !duplicates.is_empty() => {
                        // Get network name for warning message
                        let network_name = match state.dhcp_store.get_network(network_id).await {
                            Ok(network) => network.name,
                            Err(_) => format!("network {}", network_id),
                        };

                        nic.disabled = true;
                        nic.warning_label = Some(format!("Duplicate MAC on {}", network_name));

                        // Log warning with all duplicate devices
                        let duplicate_list: Vec<String> = duplicates
                            .iter()
                            .map(|(dev_uuid, iface)| format!("{}:{}", dev_uuid, iface))
                            .collect();
                        log::warn!(
                            "Duplicate MAC {} detected on network '{}' for device {} interface {}. \
                             Also found on: {}",
                            nic.mac_address,
                            network_name,
                            uuid,
                            nic.interface_name,
                            duplicate_list.join(", ")
                        );
                    }
                    Ok(_) => {
                        // No duplicates - ensure interface is not disabled
                        nic.disabled = false;
                        nic.warning_label = None;
                    }
                    Err(e) => {
                        warn!(
                            "Error checking for duplicate MAC {} on network {}: {}",
                            nic.mac_address, network_id, e
                        );
                    }
                }
            }
        }

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
        for nic in &enriched_interfaces {
            if let Err(e) = state
                .director
                .complete_pending_device(&nic.mac_address, &uuid)
                .await
            {
                log::debug!(
                    "Could not complete pending device for MAC {}: {}",
                    nic.mac_address,
                    e
                );
            }
        }
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
    extract::Path(uuid): extract::Path<String>,
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
        let test_uuid = "550e8400-e29b-41d4-a716-446655440001";

        {
            state
                .director
                .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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
        assert!(body_str.contains("chain http://localhost/cnc/ipxe?uuid={uuid}&mac={netX/mac}"));
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
        let test_uuid = "550e8400-e29b-41d4-a716-446655440010";
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
        assert!(state.director.device_exists(test_uuid).await.unwrap());

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
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();
        state.director.create_plan(&plan).await.unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

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

    #[tokio::test]
    async fn test_automatic_discovery_on_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440099";

        // Verify device doesn't exist yet
        assert!(!state.director.device_exists(test_uuid).await.unwrap());

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
        assert!(state.director.device_exists(test_uuid).await.unwrap());

        // Device should be in "new" state
        let lifecycle = state
            .director
            .get_device_lifecycle(test_uuid)
            .await
            .unwrap();
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Device should have an active discovery plan with 2 actions
        let active_plan = state
            .director
            .get_active_plan_for_device(test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0].action_type, "discover_hardware");
        assert_eq!(plan.actions[1].action_type, "configure_bmc");

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
        let test_uuid = "550e8400-e29b-41d4-a716-446655440098";

        // Register device and start discovery transition
        state
            .director
            .register_device(test_uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        state
            .director
            .start_lifecycle_transition(test_uuid, crate::lifecycle::DeviceLifecycle::Unprovisioned)
            .await
            .unwrap();

        // Simulate agent updating attributes
        let update_payload = UpdateAttributesQuery {
            uuid: test_uuid.to_string(),
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
        let device = state.director.get_device(test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("manufacturer").unwrap().as_str(),
            Some("Dell Inc.")
        );

        // Simulate agent reporting success for first action (discover_hardware)
        let success_payload = ActionStatusQuery {
            uuid: test_uuid.to_string(),
        };

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
            .get_device_lifecycle(test_uuid)
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
            .get_device_lifecycle(test_uuid)
            .await
            .unwrap();
        assert_eq!(
            lifecycle,
            Some(crate::lifecycle::DeviceLifecycle::Unprovisioned)
        );

        // Verify no active plan
        let active_plan = state
            .director
            .get_active_plan_for_device(test_uuid)
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
