mod boot_files;
mod device_registration;
mod install_script;
mod ipxe_scripts;
mod network_processing;
mod osm_files;
mod poll;

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
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

use crate::http::error::Error;
use crate::{
    dhcp,
    director::{Director, NetworkInterface},
    http::AppState,
};
use common::device_attributes::{BmcConfig, DeviceAttributes};

use ipxe_scripts::{build_response, generate_uuid_redirect};

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
        .route(
            "/cnc/osm/{module}/{version}/{os_dir}/{*file}",
            get(osm_files::osm_file_handler),
        )
        .route("/cnc/update_attributes", post(update_attributes))
        .route("/cnc/action_success", post(action_success))
        .route("/cnc/action_failed", post(action_failed))
        .route("/cnc/devices/{uuid}/bmc_config", get(get_bmc_config))
        .route("/cnc/devices/{uuid}/disk_layout", get(get_disk_layout))
        .route("/cnc/poll", get(poll::poll_handler))
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

    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(Error::ServerInternalError)?;
    // This handler can register devices and auto-start discovery transitions,
    // which issue the OOB power kick — it must carry the configured PowerConfig.
    let director = Director::with_power_config(&conn, state.power_config);

    // Resolve MAC address from parameter or DHCP lookup
    let mac_address =
        device_registration::resolve_mac_address(&conn, params.mac.as_ref(), addr).await;

    // Discovery Support
    // For devices that don't exist, determine if the device should be created.
    // TODO: Please make this better.
    if !director.device_exists(&uuid).await?
        && let Some(mac) = &mac_address
    {
        if director.find_pending_device_by_mac(mac).await?.is_some() {
            info!("Found pending device {}. Starting discovery.", uuid);
            device_registration::register_and_start_discovery(
                &conn,
                &uuid,
                Some(mac),
                state.power_config,
            )
            .await;
        } else {
            // Device is not pending. Check to see if the network has autodiscovery enabled.
            let dhcp_lease = dhcp::store::get_lease_by_mac(&conn, mac).await?;
            if let Some(lease) = dhcp_lease {
                if let Some(netid) = lease.network_id {
                    let dhcp_network = dhcp::store::get_network(&conn, netid).await?;

                    // Register device if it doesn't exist and automatically start discovery
                    if dhcp_network.enable_autodiscovery {
                        info!(
                            "Found new device {} on network with autodiscovery enabled. Adopting and starting discovery.",
                            uuid
                        );
                        device_registration::register_and_start_discovery(
                            &conn,
                            &uuid,
                            Some(mac),
                            state.power_config,
                        )
                        .await;
                    }
                } else {
                    warn!("DHCP Lease does not have a network ID")
                }
            } else {
                warn!("MAC {:?} does not have a DHCP lease", mac)
            }
        }
    } else {
        warn!("Missing MAC address")
    }

    // Handle boot event - advance plan if current action supports it
    // This must happen before we check the boot target, because on_boot() might advance the plan
    if let Err(e) = director.on_boot(&uuid).await {
        log::warn!("Failed to handle boot event for {}: {:?}", uuid, e);
    }

    // Store network info from DHCP lease (reuse the MAC address lookup from above)
    if let Some(mac) = &mac_address {
        if let Ok(Some(lease)) = dhcp::store::get_lease_by_mac(&conn, mac).await {
            log::info!(
                "Found DHCP lease for device {}: MAC {} IP {}",
                uuid,
                lease.mac_address,
                lease.ip_address
            );

            // Update IP address for the interface with this MAC
            // This will also create the interface if it doesn't exist yet
            if let Err(e) = director
                .set_device_ip_address(&uuid, &lease.ip_address, &lease.mac_address)
                .await
            {
                warn!("Couldn't store IP address for device {uuid}: {e}");
            }
        }
    } else {
        log::debug!("No MAC address available for device {}", uuid);
    }

    // Non-fatal. If the boot target can't be found, redirect loop back here to try again
    let boot_target = match director
        .next_boot_target(&uuid, state.unprovisioned_sleep_secs)
        .await
    {
        Ok(x) => x,
        Err(e) => {
            warn!("Couldn't get boot target from director for {uuid}: {e}");
            return Ok(generate_uuid_redirect(&root_url));
        }
    };

    let ipxe_script = boot_target.to_ipxe_script(&root_url, Some(&uuid)).await?;

    log::debug!("cnc/ipxe: returning script for {}:\n{}", uuid, ipxe_script);

    Ok(build_response(ipxe_script))
}

async fn install_script_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IpxeQuery>,
    Host(host): Host,
) -> Result<Response<String>, Error> {
    let root_url = format!("http://{host}");
    let uuid = params
        .uuid
        .ok_or_else(|| Error::BadRequest("Missing uuid parameter".to_string()))?;

    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(Error::ServerInternalError)?;

    install_script::render_for_device(
        &conn,
        &state.image_store,
        state.bundled_osm_path.as_deref(),
        &uuid,
        &root_url,
    )
    .await
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
    attributes: DeviceAttributes,
}

#[axum::debug_handler]
async fn update_attributes(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<UpdateAttributesQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;
    let incoming_attributes = payload.attributes;

    // Serialize incoming DeviceAttributes to JSON map for storage
    // This ensures type safety at the API boundary
    let mut attributes_json = match serde_json::to_value(&incoming_attributes) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => {
            warn!("Failed to serialize device attributes for {}", uuid);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Remove null values — null from the agent means "not provided", not "clear this field".
    // Without this, optional fields like `hostname` (set by register_device) would be
    // overwritten with null when the agent submits a hardware scan that omits them.
    attributes_json.retain(|_, v| !v.is_null());

    let conn = match state.connection_factory.open().await {
        Ok(conn) => conn,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let director = Director::new(&conn);

    // Store the attributes as provided by the agent
    if let Err(e) = director.update_attributes(&uuid, attributes_json).await {
        warn!("Couldn't update attributes for {uuid}: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // If network_interfaces were provided, backfill IP addresses from DHCP leases
    if !incoming_attributes.network_interfaces.is_empty() {
        // Parse interfaces from the typed struct
        let interfaces: Vec<NetworkInterface> = incoming_attributes
            .network_interfaces
            .iter()
            .filter_map(|nic| {
                match serde_json::from_value::<NetworkInterface>(serde_json::to_value(nic).ok()?) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        warn!("Failed to convert network interface: {}", e);
                        None
                    }
                }
            })
            .collect();

        // Enrich interfaces with DHCP lease information
        let mut enriched_interfaces =
            network_processing::enrich_interfaces_with_dhcp_info(&conn, interfaces).await;

        // Detect and mark duplicate MACs on the same network
        network_processing::detect_and_mark_duplicates(&conn, &uuid, &mut enriched_interfaces)
            .await;

        // Update the device with IP-enriched network interfaces
        if !enriched_interfaces.is_empty()
            && let Err(e) = director
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
            &conn,
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
    /// Optional plan ID echoed back by the agent from the poll response.
    ///
    /// When present, rack-director verifies the ID matches the currently
    /// active plan before applying the success report. This prevents stale
    /// reports from a previous (cancelled) plan from corrupting a new plan.
    plan_id: Option<i64>,
}

#[derive(Deserialize, Serialize)]
struct ActionFailedQuery {
    uuid: Uuid,
    error_message: String,
    /// Optional plan ID echoed back by the agent from the poll response.
    ///
    /// When present, rack-director verifies the ID matches the currently
    /// active plan before applying the failure report. This prevents stale
    /// reports from a previous (cancelled) plan from corrupting a new plan.
    plan_id: Option<i64>,
}

#[axum::debug_handler]
async fn action_success(
    State(state): State<Arc<AppState>>,
    extract::Json(payload): extract::Json<ActionStatusQuery>,
) -> Result<NoContent, StatusCode> {
    let uuid = payload.uuid;
    let plan_id = payload.plan_id;

    let conn = match state.connection_factory.open().await {
        Ok(conn) => conn,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    // Plan advancement completes lifecycle transitions; carry the configured
    // PowerConfig so any OOB power operation on this path honours the CLI flags.
    let director = Director::with_power_config(&conn, state.power_config);

    match director.mark_action_success(&uuid, plan_id).await {
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
    let plan_id = payload.plan_id;

    let conn = match state.connection_factory.open().await {
        Ok(conn) => conn,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    // Plan advancement completes lifecycle transitions; carry the configured
    // PowerConfig so any OOB power operation on this path honours the CLI flags.
    let director = Director::with_power_config(&conn, state.power_config);

    match director
        .mark_action_failed(&uuid, &error_message, plan_id)
        .await
    {
        Ok(_) => Ok(NoContent),
        Err(e) => {
            warn!("Couldn't mark action failed for {uuid}: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Generate a random 16-character password for BMC configuration
///
/// The password contains a mix of uppercase letters, lowercase letters,
/// numbers, and special characters to meet typical BMC security requirements.
fn generate_bmc_password() -> String {
    use rand::Rng;
    const CHARSET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
    let mut rng = rand::thread_rng();
    (0..16)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Get BMC configuration for a device
///
/// This endpoint returns the BMC configuration stored in the device's attributes.
/// The configuration includes static IP settings and credentials that will be
/// applied to the BMC by the rack-agent.
///
/// If no BMC configuration exists or if credentials are missing, they will be
/// auto-generated with username "RACKDIRECTOR" and a random 16-character password.
#[axum::debug_handler]
async fn get_bmc_config(
    State(state): State<Arc<AppState>>,
    extract::Path(uuid): extract::Path<Uuid>,
) -> Result<extract::Json<BmcConfig>, StatusCode> {
    let conn = match state.connection_factory.open().await {
        Ok(conn) => conn,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let director = Director::new(&conn);

    // Get device
    let mut device = match director.get_device(&uuid).await {
        Ok(device) => device,
        Err(e) => {
            warn!("Failed to get device {}: {}", uuid, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    // Extract or auto-generate BMC config
    let mut bmc_config = device.attributes.bmc_config.clone().unwrap_or_else(|| {
        log::info!("No BMC config for device {}, creating default config", uuid);
        BmcConfig {
            ip_address_source: "dhcp".to_string(),
            ip_address: None,
            netmask: None,
            gateway: None,
            username: None,
            password: None,
        }
    });

    // Auto-generate credentials if missing
    let mut needs_update = false;
    if bmc_config.username.is_none() {
        log::info!("Auto-generating BMC username for device {}", uuid);
        bmc_config.username = Some("RACKDIRECTOR".to_string());
        needs_update = true;
    }
    if bmc_config.password.is_none() {
        log::info!("Auto-generating BMC password for device {}", uuid);
        bmc_config.password = Some(generate_bmc_password());
        needs_update = true;
    }

    // Save updated config back to device attributes if we generated credentials
    if needs_update {
        log::info!("Saving auto-generated BMC credentials for device {}", uuid);
        device.attributes.bmc_config = Some(bmc_config.clone());

        // Serialize DeviceAttributes to JSON map
        let attributes_json = match serde_json::to_value(&device.attributes) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => {
                warn!("Failed to serialize device attributes for {}", uuid);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

        if let Err(e) = director.update_attributes(&uuid, attributes_json).await {
            warn!("Failed to save BMC config for device {}: {}", uuid, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

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

/// Get resolved disk layout for a device
///
/// Returns the disk layout from the device's assigned role, with platform labels
/// resolved to actual device paths if the device has a platform assigned.
#[axum::debug_handler]
async fn get_disk_layout(
    State(state): State<Arc<AppState>>,
    extract::Path(uuid): extract::Path<Uuid>,
) -> Result<extract::Json<common::disk_layout::DiskLayout>, Error> {
    let conn = state
        .connection_factory
        .open()
        .await
        .map_err(Error::ServerInternalError)?;
    let director = Director::new(&conn);

    // Get device
    let device = director
        .get_device(&uuid)
        .await
        .map_err(|_| Error::NotFound(format!("Device {} not found", uuid)))?;

    // Get role_id from device
    let role_id = device
        .role_id
        .ok_or_else(|| Error::BadRequest("Device has no role assigned".to_string()))?;

    // Get role's disk_layout
    let role = crate::roles::store::get(&conn, role_id)
        .await
        .map_err(Error::ServerInternalError)?;

    let layout = role.disk_layout;

    // Check if layout uses labels
    if crate::disk_layout::layout_uses_labels(&layout) {
        // Need platform to resolve labels
        let platform_id = device.platform_id.ok_or_else(|| {
            Error::BadRequest(
                "Disk layout uses platform labels but device has no platform assigned".to_string(),
            )
        })?;

        let platform = crate::platforms::store::get(&conn, platform_id)
            .await
            .map_err(Error::ServerInternalError)?;

        let resolved = crate::disk_layout::resolve_disk_layout(
            &layout,
            &platform.attributes,
            &device.attributes,
        )
        .map_err(Error::ServerInternalError)?;

        Ok(extract::Json(resolved))
    } else {
        // No labels, return as-is
        Ok(extract::Json(layout))
    }
}

#[cfg(test)]
mod tests {
    use crate::{director::Director, storage::ImageStore};

    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }

    fn test_mac(suffix: u16) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            0x01, 0x02, 0x03, 0x04, 0x05, suffix
        )
    }

    async fn setup_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        // Enable test logs
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Note: No default network is created. Tests that need networks should create them explicitly.

        // Create image store for testing
        let image_store = ImageStore::memory();

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
        std::fs::write(boot_files_path.join("snponly.efi"), b"mock snponly.efi").unwrap();
        std::fs::write(boot_files_path.join("undionly.kpxe"), b"mock undionly.kpxe").unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let conn: Arc<dyn crate::database::ConnectionFactory> = Arc::new(
            crate::database::DatabaseConnectionFactory::new(db_path.clone()),
        );
        // Run migrations; drop the returned connection (file-backed DB persists)
        let _ = crate::database::run_migrations(conn.as_ref())
            .await
            .unwrap();

        let state = Arc::new(AppState {
            connection_factory: conn,
            image_store: Arc::new(image_store),
            agent_images_path,
            boot_file_provider,
            dhcp: crate::dhcp::DhcpControl::noop(),
            unprovisioned_sleep_secs: 600,
            bundled_osm_path: None,
            power_config: crate::director::power::PowerConfig::default(),
        });
        (state, temp_dir)
    }

    /// Open a database connection for test assertions.
    ///
    /// Usage: `let db = test_db(&state).await; let director = Director::new(&db);`
    async fn test_db(state: &AppState) -> crate::database::Connection {
        state.connection_factory.open().await.unwrap()
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(state: &AppState, autodiscovery: bool) -> i64 {
        let conn = Arc::new(state.connection_factory.open().await.unwrap());
        let network = crate::dhcp::store::create_network(
            &conn,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string()],
            86400,
            None,
            autodiscovery,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(&conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_ipxe_new_device_unknown_network() {
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
        assert!(body_str.contains("#!ipxe"), "Did not return an ipxe script");
        // Unknown devices are not provisioned, so they should sleep and retry rather than
        // attempt to boot local disk (which would have no OS).
        assert!(
            body_str.contains("sleep") && body_str.contains("reboot"),
            "Unprovisioned/unknown devices must sleep and retry, got: {body_str}"
        );
    }

    #[tokio::test]
    async fn test_ipxe_known_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let uuid = test_uuid(1);

        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&uuid, crate::director::Architecture::X86_64)
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
        // A newly registered device is in the New lifecycle state with no active plan.
        // It should sleep and retry rather than boot local disk (no OS is installed yet).
        assert!(
            body_str.contains("sleep") && body_str.contains("reboot"),
            "New device with no active plan must sleep and retry, got: {body_str}"
        );
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
    async fn test_ipxe_handler_pending() {
        let (state, _temp_dir) = setup_test_state().await;
        let network_id = create_test_network(&state, false).await;
        let test_uuid = test_uuid(0x10);
        let test_mac = &test_mac(0x00);

        // Create a pending device for this MAC
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .create_pending_device(test_mac, network_id)
                .await
                .unwrap();
        }

        // Verify pending device exists
        let pending_id = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .find_pending_device_by_mac(test_mac)
                .await
                .unwrap()
        };
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
        assert!({
            let conn = test_db(&state).await;
            Director::new(&conn)
                .device_exists(&test_uuid)
                .await
                .unwrap()
        });

        // Pending device should be completed (removed from pending_devices table)
        let pending_id = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .find_pending_device_by_mac(test_mac)
                .await
                .unwrap()
        };
        assert!(pending_id.is_none(), "Pending device should be completed");
    }

    #[tokio::test]
    async fn test_action_success() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x03);

        // Create a test plan — InstallOs must not be first; prepend DiscoverHardware
        let actions = vec![
            crate::plans::Action::DiscoverHardware,
            crate::plans::Action::InstallOs,
        ];
        let plan = crate::plans::Plan::new(test_uuid, actions);

        // Register device and create plan
        {
            let conn = test_db(&state).await;
            let director = Director::new(&conn);
            director
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
            director.create_plan(&plan).await.unwrap();
        }

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionStatusQuery {
            uuid: test_uuid,
            plan_id: None,
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
        let test_uuid = test_uuid(0x04);

        // Create a test plan — InstallOs must not be first; prepend DiscoverHardware
        let actions = vec![
            crate::plans::Action::DiscoverHardware,
            crate::plans::Action::InstallOs,
        ];
        let plan = crate::plans::Plan::new(test_uuid, actions);

        // Register device and create plan
        {
            let conn = test_db(&state).await;
            let director = Director::new(&conn);
            director
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
            director.create_plan(&plan).await.unwrap();
        }

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionFailedQuery {
            uuid: test_uuid,
            error_message: "Installation failed".to_string(),
            plan_id: None,
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
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        let payload = ActionStatusQuery {
            uuid: test_uuid,
            plan_id: None,
        };

        let request = Request::builder()
            .method("POST")
            .uri("/cnc/action_success")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // No active plan is now a silent no-op (plan may have been cancelled while agent
        // was in-flight), so the handler returns 204 NoContent rather than 500.
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_automatic_discovery_on_new_device() {
        let (state, _temp_dir) = setup_test_state().await;
        let network_id = create_test_network(&state, true).await;
        let test_uuid = test_uuid(0x99);
        let test_mac = test_mac(0x00);
        {
            let conn = Arc::new(state.connection_factory.open().await.unwrap());
            crate::dhcp::store::create_or_update_lease_with_network(
                &conn,
                &test_mac,
                &Ipv4Addr::UNSPECIFIED,
                None,
                crate::dhcp::LeaseState::Active,
                300,
                network_id,
            )
            .await
            .unwrap();
        }

        // Verify device doesn't exist yet
        assert!({
            let conn = test_db(&state).await;
            !Director::new(&conn)
                .device_exists(&test_uuid)
                .await
                .unwrap()
        });

        let app = routes(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
        ));

        // First boot - device registers and discovery starts
        let request = Request::builder()
            .header("Host", "localhost")
            .uri(format!("/cnc/ipxe?uuid={}&mac={}", test_uuid, test_mac))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Device should now exist
        assert!({
            let conn = test_db(&state).await;
            Director::new(&conn)
                .device_exists(&test_uuid)
                .await
                .unwrap()
        });

        // Device should be in "new" state
        let lifecycle = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .get_device_lifecycle(&test_uuid)
                .await
                .unwrap()
        };
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Device should have an active discovery plan with 2 actions
        let active_plan = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .get_active_plan_for_device(&test_uuid)
                .await
                .unwrap()
        };
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
        assert!(
            body_str.contains("/cnc/agent-images/vmlinuz"),
            "iPXE script is missing vmlinuz:\n{}",
            body_str
        );
        assert!(
            body_str.contains("/cnc/agent-images/initramfs.img"),
            "iPXE script is missing initramfs.img:\n{}",
            body_str
        );
        assert!(
            body_str.contains("rackdirector.url="),
            "iPXE script is missing rackdirector.url:\n{}",
            body_str
        );
    }

    #[tokio::test]
    async fn test_discovery_completion_flow() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x98);

        // Register device and start discovery transition
        {
            let conn = test_db(&state).await;
            let director = Director::new(&conn);
            director
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
            director
                .start_lifecycle_transition(
                    &test_uuid,
                    crate::lifecycle::DeviceLifecycle::Unprovisioned,
                )
                .await
                .unwrap();
        }

        // Simulate agent updating attributes
        let update_payload = UpdateAttributesQuery {
            uuid: test_uuid,
            attributes: DeviceAttributes {
                manufacturer: Some("Dell Inc.".to_string()),
                product_name: Some("PowerEdge R640".to_string()),
                ..Default::default()
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
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert_eq!(
            device.attributes.manufacturer.as_ref().unwrap(),
            "Dell Inc."
        );

        // Simulate agent reporting success for first action (discover_hardware)
        let success_payload = ActionStatusQuery {
            uuid: test_uuid,
            plan_id: None,
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
        let lifecycle = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .get_device_lifecycle(&test_uuid)
                .await
                .unwrap()
        };
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
        let lifecycle = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .get_device_lifecycle(&test_uuid)
                .await
                .unwrap()
        };
        assert_eq!(
            lifecycle,
            Some(crate::lifecycle::DeviceLifecycle::Unprovisioned)
        );

        // Verify no active plan
        let active_plan = {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .get_active_plan_for_device(&test_uuid)
                .await
                .unwrap()
        };
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

    // ========== BMC Password Generation Tests ==========

    #[test]
    fn test_generate_bmc_password_length() {
        let password = generate_bmc_password();
        assert_eq!(password.len(), 16);
    }

    #[test]
    fn test_generate_bmc_password_charset() {
        let password = generate_bmc_password();
        const CHARSET: &str =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";

        // All characters should be from the allowed charset
        for ch in password.chars() {
            assert!(
                CHARSET.contains(ch),
                "Password contains invalid character: {}",
                ch
            );
        }
    }

    #[test]
    fn test_generate_bmc_password_randomness() {
        // Generate multiple passwords and verify they're different
        let password1 = generate_bmc_password();
        let password2 = generate_bmc_password();
        let password3 = generate_bmc_password();

        // It's extremely unlikely (but technically possible) for these to be equal
        assert_ne!(password1, password2);
        assert_ne!(password2, password3);
        assert_ne!(password1, password3);
    }

    #[tokio::test]
    async fn test_get_bmc_config_auto_generates_credentials() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x60);

        // Register device
        {
            let conn = test_db(&state).await;
            let director = Director::new(&conn);
            director
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();

            // Set BMC config without credentials
            let mut attributes = serde_json::Map::new();
            attributes.insert(
                "bmc_config".to_string(),
                serde_json::json!({
                    "ip_address_source": "dhcp"
                }),
            );

            director
                .update_attributes(&test_uuid, attributes)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Get BMC config via CNC endpoint
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/bmc_config", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let bmc_config: common::device_attributes::BmcConfig =
            serde_json::from_slice(&body).unwrap();

        // Verify credentials were auto-generated
        assert_eq!(bmc_config.username, Some("RACKDIRECTOR".to_string()));
        assert!(bmc_config.password.is_some());
        assert_eq!(bmc_config.password.unwrap().len(), 16);

        // Verify credentials were saved to device
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert_eq!(
            device.attributes.bmc_config.as_ref().unwrap().username,
            Some("RACKDIRECTOR".to_string())
        );
        assert!(
            device
                .attributes
                .bmc_config
                .as_ref()
                .unwrap()
                .password
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_get_bmc_config_preserves_existing_credentials() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x61);

        // Register device
        {
            let conn = test_db(&state).await;
            let director = Director::new(&conn);
            director
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();

            // Set BMC config with existing credentials
            let mut attributes = serde_json::Map::new();
            attributes.insert(
                "bmc_config".to_string(),
                serde_json::json!({
                    "ip_address_source": "static",
                    "ip_address": "10.0.1.100",
                    "netmask": "255.255.255.0",
                    "gateway": "10.0.1.1",
                    "username": "existing_user",
                    "password": "existing_pass"
                }),
            );

            director
                .update_attributes(&test_uuid, attributes)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Get BMC config via CNC endpoint
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/bmc_config", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let bmc_config: common::device_attributes::BmcConfig =
            serde_json::from_slice(&body).unwrap();

        // Verify existing credentials were preserved
        assert_eq!(bmc_config.username, Some("existing_user".to_string()));
        assert_eq!(bmc_config.password, Some("existing_pass".to_string()));
    }

    #[tokio::test]
    async fn test_get_bmc_config_creates_config_if_missing() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x62);

        // Register device without BMC config
        {
            let conn = test_db(&state).await;
            Director::new(&conn)
                .register_device(&test_uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let app = routes(state.clone());

        // Get BMC config via CNC endpoint
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/bmc_config", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Parse response
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let bmc_config: common::device_attributes::BmcConfig =
            serde_json::from_slice(&body).unwrap();

        // Verify config was created with auto-generated credentials
        assert_eq!(bmc_config.ip_address_source, "dhcp");
        assert_eq!(bmc_config.username, Some("RACKDIRECTOR".to_string()));
        assert!(bmc_config.password.is_some());

        // Verify config was saved to device
        let device = {
            let conn = test_db(&state).await;
            Director::new(&conn).get_device(&test_uuid).await.unwrap()
        };
        assert!(device.attributes.bmc_config.is_some());
    }

    #[tokio::test]
    async fn test_get_disk_layout_success_with_paths() {
        // Device with role that uses paths only (no platform needed)
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x70);

        let conn = test_db(&state).await;
        let director = Director::new(&conn);

        // Register device
        director
            .register_device(&test_uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // Create role with path-based layout
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![common::disk_layout::PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: true,
        };
        let role = crate::roles::store::create(
            &conn,
            "test-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Assign role to device
        director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await
            .unwrap();

        let app = routes(state.clone());
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/disk_layout", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: common::disk_layout::DiskLayout = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.disks.len(), 1);
        assert_eq!(
            result.disks[0].device,
            "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        );
        assert!(
            result.wipe_all_disks,
            "wipe_all_disks should be true as stored in the role layout"
        );
    }

    #[tokio::test]
    async fn test_get_disk_layout_no_role() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x71);

        // Register device but don't assign a role
        let conn = test_db(&state).await;
        Director::new(&conn)
            .register_device(&test_uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let app = routes(state.clone());
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/disk_layout", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_disk_layout_not_found() {
        let (state, _temp_dir) = setup_test_state().await;
        let fake_uuid = test_uuid(0x72);

        let app = routes(state.clone());
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/disk_layout", fake_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Label resolution (Phase 4): a device with platform labels and matching device
    /// disks in its attributes returns HTTP 200 with the label resolved to the device's
    /// own by-path string.
    #[tokio::test]
    async fn test_get_disk_layout_labels_resolved_via_canonical_position() {
        let (state, _temp_dir) = setup_test_state().await;
        let test_uuid = test_uuid(0x74);

        let conn = test_db(&state).await;
        let director = Director::new(&conn);

        director
            .register_device(&test_uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // Create platform with ROOT label (480 GB SSD)
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![crate::platforms::PlatformDisk {
                size_gb: 480,
                disk_type: crate::platforms::DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };
        let platform =
            crate::platforms::store::create(&conn, "Test Platform", None, &platform_attrs, None)
                .await
                .unwrap();
        director
            .assign_platform_to_device(&test_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Store device attributes including a disk with a by-path that matches the
        // platform class (SSD, 480 GB).  This simulates what the agent reports after
        // hardware discovery.
        let device_attrs = common::device_attributes::DeviceAttributes {
            disks: vec![common::device_attributes::DiskInfo {
                name: "sda".to_string(),
                size: Some(480),
                disk_type: Some(common::device_attributes::DiskType::Ssd),
                path: Some("/dev/disk/by-path/pci-0000:05:00.0-ata-1".to_string()),
                model: None,
                serial: None,
                vendor: None,
                uuid: None,
            }],
            ..Default::default()
        };
        let attrs_json = serde_json::to_value(&device_attrs)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        director
            .update_attributes(&test_uuid, attrs_json)
            .await
            .unwrap();

        // Create role with label-based layout
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![common::disk_layout::PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: true,
        };
        let role = crate::roles::store::create(
            &conn,
            "label-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await
            .unwrap();

        let app = routes(state.clone());
        let request = Request::builder()
            .uri(format!("/cnc/devices/{}/disk_layout", test_uuid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: common::disk_layout::DiskLayout = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.disks.len(), 1);
        assert_eq!(
            result.disks[0].device, "/dev/disk/by-path/pci-0000:05:00.0-ata-1",
            "ROOT label should resolve to the device's own by-path"
        );
        assert!(
            result.wipe_all_disks,
            "wipe_all_disks should be true as stored in the role layout, even after label resolution"
        );
    }
}

/// Shared test helpers for cnc submodule tests.
///
/// Provides a common `setup_test_state` that accepts a `DatabaseConnectionFactory`
/// so each test can supply a unique in-memory DB URI via `test_connection_factory!()`.
/// The returned `Connection` must be kept alive for the duration of the test to
/// prevent the in-memory database from being destroyed.
#[cfg(test)]
pub(super) mod test_helpers {
    use std::sync::Arc;
    use tempfile::TempDir;

    use crate::{
        database::{self, DatabaseConnectionFactory},
        http::AppState,
        storage::ImageStore,
    };

    pub(super) async fn setup_test_state(
        factory: DatabaseConnectionFactory,
    ) -> (Arc<AppState>, TempDir, database::Connection) {
        let conn_factory: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        let migration_conn = database::run_migrations(conn_factory.as_ref())
            .await
            .unwrap();

        let temp_dir = tempfile::tempdir().unwrap();

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let image_store = ImageStore::memory();

        let state = Arc::new(AppState {
            connection_factory: conn_factory,
            image_store: Arc::new(image_store),
            agent_images_path,
            boot_file_provider,
            dhcp: crate::dhcp::DhcpControl::noop(),
            unprovisioned_sleep_secs: 600,
            bundled_osm_path: None,
            power_config: crate::director::power::PowerConfig::default(),
        });

        (state, temp_dir, migration_conn)
    }
}
