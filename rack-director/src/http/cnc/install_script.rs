use crate::http::error::Error;
use crate::templates;
use crate::{database, dhcp, director::Director, roles};
use anyhow::anyhow;
use axum::{
    http::{StatusCode, header},
    response::Response,
};
use common::Ipv4Subnet;
use futures::TryStreamExt;
use uuid::Uuid;

/// Renders an install script template for a specific device.
///
/// This function orchestrates the complete install script rendering process:
/// 1. Retrieves device information and role assignment
/// 2. Fetches OS and architecture configuration
/// 3. Loads the install script template — from disk for bundled OSMs, or from
///    the image store for uploaded OSMs
/// 4. Gathers device network information from DHCP
/// 5. Renders the template with device context
///
/// # Arguments
/// * `conn` - An open database connection
/// * `image_store` - Image store for downloading script templates (uploaded OSMs)
/// * `bundled_osm_path` - Root directory for bundled OSM files on disk, if configured
/// * `device_uuid` - UUID of the device requesting the install script
/// * `root_url` - Base URL used in rendered template variables
///
/// # Returns
/// * `Ok(Response)` - HTTP response containing the rendered install script
/// * `Err(Error)` - Error if device not found, missing role, template issues, etc.
pub async fn render_for_device(
    conn: &database::Connection,
    image_store: &crate::storage::ImageStore,
    bundled_osm_path: Option<&std::path::Path>,
    device_uuid: &Uuid,
    root_url: &str,
) -> Result<Response<String>, Error> {
    let director = Director::new(conn);

    // Get device
    let device = director
        .get_device(device_uuid)
        .await
        .map_err(Error::ServerInternalError)?;

    // Get device role
    let role_id = crate::director::store::get_device_role_id(conn, device_uuid)
        .await
        .map_err(Error::ServerInternalError)?
        .ok_or_else(|| Error::NotFound("Device has no role assigned".to_string()))?;

    let role = roles::store::get(conn, role_id)
        .await
        .map_err(Error::ServerInternalError)?;

    // Resolve OS from OSM
    let resolved = crate::osm::resolve_os(
        conn,
        &role.osm_module,
        &role.os_name,
        &role.os_release,
        &role.os_arch,
    )
    .await
    .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Failed to resolve OS: {}", e)))?;

    // Load install script template — bundled modules are served from disk,
    // uploaded modules from the image store.
    let template = if resolved.module.source == "bundled" {
        load_bundled_template(bundled_osm_path, &resolved).await?
    } else {
        load_stored_template(image_store, &resolved).await?
    };

    // Build per-NIC network context and the legacy primary NetworkInfo.
    let (network_info, nic_contexts) =
        build_network_contexts(conn, device_uuid, &device.attributes.network_interfaces).await?;

    // Get device attributes
    let hostname = device.attributes.hostname.clone();
    let boot_mode = device.attributes.boot_mode;

    let device_info = templates::DeviceInfo {
        uuid: *device_uuid,
        hostname,
        boot_mode,
    };

    // Get resolved disk layout for template context.
    // If the layout uses platform labels, resolve them to actual device paths
    // using the device's assigned platform attributes.
    let owned_layout;
    let resolved_disk_layout: &common::disk_layout::DiskLayout =
        if crate::disk_layout::layout_uses_labels(&role.disk_layout) {
            let platform_id = device.platform_id.ok_or_else(|| {
                Error::BadRequest(
                    "Disk layout uses platform labels but device has no platform assigned"
                        .to_string(),
                )
            })?;
            let platform = crate::platforms::store::get(conn, platform_id)
                .await
                .map_err(Error::ServerInternalError)?;
            owned_layout = crate::disk_layout::resolve_disk_layout(
                &role.disk_layout,
                &platform.attributes,
                &device.attributes,
            )
            .map_err(Error::ServerInternalError)?;
            &owned_layout
        } else {
            &role.disk_layout
        };

    // Render with OSM function
    let rendered = templates::render_install_script_osm(
        &template,
        &device_info,
        &role.name,
        &role.disk_layout,
        &role.config_template,
        &role.os_name,
        &role.os_release,
        &network_info,
        &nic_contexts,
        resolved_disk_layout,
        root_url,
    )
    .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Template render failed: {}", e)))?;

    log::debug!("Rendered install script for {}:\n{}", device_uuid, rendered);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
}

/// Load an install script template from the on-disk bundled OSM directory.
///
/// Bundled OSMs are never uploaded to the image store — their files live only
/// on disk at `{bundled_osm_path}/{os_dir}/{install_template}`.
async fn load_bundled_template(
    bundled_osm_path: Option<&std::path::Path>,
    resolved: &crate::osm::ResolvedOs,
) -> Result<String, Error> {
    let path = bundled_osm_path.ok_or_else(|| {
        Error::ServerInternalError(anyhow::anyhow!(
            "Bundled OSM '{}' requires --bundled-osm-path to be set",
            resolved.module.name
        ))
    })?;
    let disk_path = path
        .join(&resolved.os.dir_name)
        .join(&resolved.arch_config.install_template);

    // Guard against path traversal: `dir_name` and `install_template` come from
    // the database and could contain `..` components if a corrupt or malicious OSM
    // archive was accepted. Reject any path that escapes the base directory.
    if !path_is_within_base(path, &disk_path) {
        return Err(Error::ServerInternalError(anyhow::anyhow!(
            "Install template path '{}' is outside the bundled OSM directory",
            disk_path.display()
        )));
    }

    let bytes = tokio::fs::read(&disk_path).await.map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!(
            "Failed to read bundled install template {}: {}",
            disk_path.display(),
            e
        ))
    })?;
    String::from_utf8(bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Template is not valid UTF-8: {}", e))
    })
}

/// Returns `true` if `path` is lexically within `base` after resolving all `..` components.
///
/// This is a defence-in-depth check — it catches `..`-based traversals without requiring
/// filesystem access (no symlink resolution).  A separate `canonicalize` check would be
/// needed to block symlink-based traversals, but those are not a realistic threat for
/// bundled OSM files shipped with the binary.
fn path_is_within_base(base: &std::path::Path, path: &std::path::Path) -> bool {
    use std::path::Component;
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c),
        }
    }
    normalized.starts_with(base)
}

/// Load an install script template from the image store (uploaded OSMs).
async fn load_stored_template(
    image_store: &crate::storage::ImageStore,
    resolved: &crate::osm::ResolvedOs,
) -> Result<String, Error> {
    let template_path = resolved.install_template_storage_path();
    let (stream, _size) = image_store.download(&template_path).await.map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Failed to download template: {}", e))
    })?;
    let script_bytes = stream
        .try_fold(Vec::new(), |mut vec, data| async move {
            vec.extend_from_slice(&data);
            Ok(vec)
        })
        .await
        .map_err(|e| {
            log::warn!("Error retrieving install script {}: {}", template_path, e);
            Error::ServerInternalError(anyhow!(""))
        })?;
    String::from_utf8(script_bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Script is not valid UTF-8: {}", e))
    })
}

/// Build per-NIC network contexts and the legacy primary `NetworkInfo` for template rendering.
///
/// For each NIC in `interfaces`, the effective network is `assigned_network_id.or(network_id)`
/// and the effective IP is `assigned_ip.clone().or(ip_address.clone())`. NICs without both
/// an effective network and an effective IP are skipped. NICs marked `disabled` (e.g. by
/// duplicate-MAC detection) are always skipped, even if they still carry an effective
/// network+IP from DHCP enrichment.
///
/// The primary NIC is the one whose MAC matches the active DHCP lease (preserving pre-existing
/// behavior). If no lease exists but at least one NIC has a configured network+IP, the first
/// such NIC becomes primary. If no usable NIC exists at all, an error equivalent to the old
/// "Device has no DHCP lease" error is returned.
///
/// Returns `(primary_network_info, all_nic_contexts)`.
async fn build_network_contexts(
    conn: &database::Connection,
    device_uuid: &Uuid,
    interfaces: &[common::device_attributes::NetworkInterface],
) -> Result<(templates::NetworkInfo, Vec<templates::NicNetworkContext>), Error> {
    // Determine which MAC has an active DHCP lease so we can mark it as primary.
    let lease_mac = find_lease_mac(conn, device_uuid).await?;

    // Build a NicNetworkContext for every NIC that has both an effective network and IP.
    let mut nic_contexts: Vec<templates::NicNetworkContext> = Vec::new();
    for iface in interfaces {
        if let Some(ctx) = build_nic_context(conn, iface, &lease_mac).await? {
            nic_contexts.push(ctx);
        }
    }

    // Promote the first NIC to primary if none was marked yet (no lease but a configured NIC exists).
    if !nic_contexts.is_empty() && !nic_contexts.iter().any(|n| n.is_primary) {
        nic_contexts[0].is_primary = true;
    }

    // Extract the primary NIC to build the legacy NetworkInfo.
    let primary = nic_contexts
        .iter()
        .find(|n| n.is_primary)
        .ok_or_else(|| Error::NotFound("Device has no DHCP lease".to_string()))?;

    let network_info = templates::NetworkInfo {
        mac_address: primary.mac_address.clone(),
        ip_address: primary.ip_address.clone(),
        gateway: primary.gateway.clone(),
        dns_servers: if primary.dns_servers.is_empty() {
            vec![]
        } else {
            primary
                .dns_servers
                .split(',')
                .map(|s| s.to_string())
                .collect()
        },
        netmask: primary.netmask.clone(),
        prefix_length: primary.prefix_length,
    };

    Ok((network_info, nic_contexts))
}

/// Returns the MAC address of the active DHCP lease for `device_uuid`, if any.
async fn find_lease_mac(
    conn: &database::Connection,
    device_uuid: &Uuid,
) -> Result<Option<String>, Error> {
    let lease = dhcp::store::find_lease_by_device_uuid(conn, device_uuid)
        .await
        .map_err(Error::ServerInternalError)?;
    Ok(lease.map(|l| l.mac_address))
}

/// Build a `NicNetworkContext` for a single NIC, or return `None` if the NIC is
/// disabled (e.g. duplicate-MAC detection) or lacks both an effective network
/// and an effective IP.
async fn build_nic_context(
    conn: &database::Connection,
    iface: &common::device_attributes::NetworkInterface,
    lease_mac: &Option<String>,
) -> Result<Option<templates::NicNetworkContext>, Error> {
    // Disabled NICs (e.g. duplicate MAC on the network) may still carry a
    // DHCP-derived network_id/ip_address from enrichment — they must never be
    // emitted into the install script or selected as primary.
    if iface.disabled {
        return Ok(None);
    }

    let effective_network_id = iface.assigned_network_id.or(iface.network_id);
    let effective_ip = iface
        .assigned_ip
        .clone()
        .or_else(|| iface.ip_address.clone());

    let (network_id, ip_address) = match (effective_network_id, effective_ip) {
        (Some(nid), Some(ip)) => (nid, ip),
        _ => return Ok(None),
    };

    let network = dhcp::store::get_network(conn, network_id)
        .await
        .map_err(Error::ServerInternalError)?;

    let subnet: Ipv4Subnet = network
        .subnet
        .parse()
        .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Invalid subnet: {}", e)))?;

    let is_primary = lease_mac
        .as_deref()
        .map(|m| m.eq_ignore_ascii_case(&iface.mac_address))
        .unwrap_or(false);

    Ok(Some(templates::NicNetworkContext {
        interface_name: iface.interface_name.clone(),
        mac_address: iface.mac_address.clone(),
        ip_address,
        netmask: subnet.netmask().to_string(),
        prefix_length: subnet.subnet(),
        gateway: network.gateway.clone(),
        dns_servers: network.dns_servers.join(","),
        is_primary,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{DatabaseConnectionFactory, run_migrations};

    // ── path_is_within_base ────────────────────────────────────────────────────

    #[test]
    fn test_path_within_base_safe_path() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("almalinux-10").join("install.ks");
        assert!(path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_dotdot_traversal_rejected() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("../escape").join("install.ks");
        assert!(!path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_deeply_nested_traversal_rejected() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("subdir/../../etc/passwd");
        assert!(!path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_exact_base_ok() {
        let base = std::path::Path::new("/bundled");
        assert!(path_is_within_base(base, base));
    }
    use crate::http::AppState;
    use crate::storage::ImageStore;
    use crate::test_database_path;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    async fn create_test_conn(path: String) -> database::Connection {
        let factory = DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        run_migrations(&factory).await.unwrap()
    }

    async fn create_test_state(
        factory: DatabaseConnectionFactory,
    ) -> (Arc<AppState>, tempfile::TempDir, database::Connection) {
        let conn: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        // Run migrations and retain the connection so the in-memory DB persists for the test
        let migration_conn = run_migrations(conn.as_ref()).await.unwrap();

        let image_store = ImageStore::memory();

        // Create temporary directories needed for agent images and boot files
        let temp_dir = tempdir().unwrap();

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

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

        (state, temp_dir, migration_conn)
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(conn: &database::Connection) -> i64 {
        let network = crate::dhcp::store::create_network(
            conn,
            "Test Network",
            "10.0.0.0/24",
            Some("10.0.0.1"),
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_build_network_contexts_no_usable_nic() {
        let conn = create_test_conn(test_database_path!()).await;

        let uuid = test_uuid();
        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // No lease, no NIC assignments — should return NotFound
        let result = build_network_contexts(&conn, &uuid, &[]).await;
        assert!(result.is_err());
        match result {
            Err(Error::NotFound(msg)) => {
                assert!(msg.contains("no DHCP lease"), "unexpected message: {}", msg);
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_build_network_contexts_with_lease() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // Create a DHCP lease
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        crate::dhcp::store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            Some(&uuid),
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Build a NIC that has DHCP-derived network/IP (simulates enriched attributes)
        let iface = common::device_attributes::NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network_id),
            assigned_network_id: None,
            assigned_ip: None,
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };

        let (network_info, nics) = match build_network_contexts(&conn, &uuid, &[iface]).await {
            Ok(v) => v,
            Err(_) => panic!("Expected Ok from build_network_contexts"),
        };

        // Primary NetworkInfo matches the lease NIC
        assert_eq!(network_info.mac_address, mac);
        assert_eq!(network_info.ip_address, "10.0.0.100");
        assert_eq!(network_info.gateway, Some("10.0.0.1".to_string()));
        assert_eq!(network_info.netmask, "255.255.255.0");
        assert_eq!(network_info.prefix_length, 24);

        // Single NIC context is marked primary
        assert_eq!(nics.len(), 1);
        assert!(nics[0].is_primary);
        assert_eq!(nics[0].mac_address, mac);
        assert_eq!(nics[0].gateway, Some("10.0.0.1".to_string()));
    }

    /// Verifies that primary-NIC selection matches the lease MAC case-insensitively:
    /// a NIC MAC stored in uppercase must still be selected as primary when the DHCP
    /// lease recorded the MAC in lowercase (or vice versa).
    #[tokio::test]
    async fn test_build_network_contexts_lease_mac_case_insensitive() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        // NIC's stored MAC is uppercase...
        let nic_mac = "AA:BB:CC:DD:EE:FF";
        // ...but the DHCP lease recorded the MAC in lowercase.
        let lease_mac = "aa:bb:cc:dd:ee:ff";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        crate::dhcp::store::create_or_update_lease_with_network(
            &conn,
            lease_mac,
            &ip,
            Some(&uuid),
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        let iface = common::device_attributes::NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: nic_mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network_id),
            assigned_network_id: None,
            assigned_ip: None,
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };

        let (network_info, nics) = match build_network_contexts(&conn, &uuid, &[iface]).await {
            Ok(v) => v,
            Err(_) => panic!("Expected Ok from build_network_contexts"),
        };

        assert_eq!(nics.len(), 1);
        assert!(
            nics[0].is_primary,
            "NIC must be selected as primary despite MAC case mismatch with the lease"
        );
        assert_eq!(network_info.mac_address, nic_mac);
    }

    /// Verifies that a NIC with `assigned_network_id` and `assigned_ip` (no DHCP lease) is
    /// treated as primary when it is the only usable NIC.
    #[tokio::test]
    async fn test_build_network_contexts_assigned_no_lease() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // NIC has operator-assigned network/IP but no DHCP lease
        let iface = common::device_attributes::NetworkInterface {
            interface_name: "eth1".to_string(),
            mac_address: mac.to_string(),
            ip_address: None,
            network_id: None,
            assigned_network_id: Some(network_id),
            assigned_ip: Some("10.0.0.50".to_string()),
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };

        let (network_info, nics) = match build_network_contexts(&conn, &uuid, &[iface]).await {
            Ok(v) => v,
            Err(_) => panic!("Expected Ok from build_network_contexts"),
        };

        assert_eq!(network_info.ip_address, "10.0.0.50");
        assert_eq!(network_info.gateway, Some("10.0.0.1".to_string()));
        assert_eq!(nics.len(), 1);
        assert!(
            nics[0].is_primary,
            "assigned-only NIC must be promoted to primary"
        );
    }

    /// Verifies that a NIC without both an effective network and IP is skipped.
    #[tokio::test]
    async fn test_build_network_contexts_skips_nic_without_ip() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let mac_with_ip = "aa:bb:cc:dd:ee:01";
        let mac_no_ip = "aa:bb:cc:dd:ee:02";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // First NIC has a full assignment
        let iface_with_ip = common::device_attributes::NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac_with_ip.to_string(),
            ip_address: None,
            network_id: None,
            assigned_network_id: Some(network_id),
            assigned_ip: Some("10.0.0.77".to_string()),
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };
        // Second NIC has no network/IP — must be skipped
        let iface_no_ip = common::device_attributes::NetworkInterface {
            interface_name: "eth1".to_string(),
            mac_address: mac_no_ip.to_string(),
            ip_address: None,
            network_id: None,
            assigned_network_id: None,
            assigned_ip: None,
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };

        let (_info, nics) =
            match build_network_contexts(&conn, &uuid, &[iface_with_ip, iface_no_ip]).await {
                Ok(v) => v,
                Err(_) => panic!("Expected Ok from build_network_contexts"),
            };

        assert_eq!(nics.len(), 1, "NIC without effective IP must be skipped");
        assert_eq!(nics[0].mac_address, mac_with_ip);
    }

    /// Verifies that a NIC marked `disabled` (e.g. by duplicate-MAC detection) is excluded
    /// from `nic_contexts` even though it still carries an effective network+IP from DHCP
    /// enrichment, and that it is never selected as primary.
    #[tokio::test]
    async fn test_build_network_contexts_skips_disabled_nic() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // NIC has a fully populated effective network+IP (as if enriched from a DHCP lease
        // before duplicate-MAC detection ran), but is disabled due to a duplicate MAC.
        let iface = common::device_attributes::NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network_id),
            assigned_network_id: None,
            assigned_ip: None,
            speed_mbps: Some(1000),
            disabled: true,
            warning_label: Some("Duplicate MAC on Test Network".to_string()),
        };

        // No DHCP lease exists and the only NIC is disabled — no usable NIC remains.
        let result = build_network_contexts(&conn, &uuid, &[iface]).await;
        match result {
            Err(Error::NotFound(msg)) => {
                assert!(msg.contains("no DHCP lease"), "unexpected message: {}", msg);
            }
            Ok(_) => panic!("Expected NotFound error but got Ok"),
            Err(_) => panic!("Expected NotFound error but got a different Error variant"),
        }
    }

    /// Verifies that when one NIC is disabled and another is usable, the disabled NIC is
    /// excluded from `nic_contexts` and the usable NIC is promoted to primary instead.
    #[tokio::test]
    async fn test_build_network_contexts_disabled_nic_not_selected_as_primary() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let disabled_mac = "aa:bb:cc:dd:ee:01";
        let usable_mac = "aa:bb:cc:dd:ee:02";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let disabled_iface = common::device_attributes::NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: disabled_mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network_id),
            assigned_network_id: None,
            assigned_ip: None,
            speed_mbps: Some(1000),
            disabled: true,
            warning_label: Some("Duplicate MAC on Test Network".to_string()),
        };
        let usable_iface = common::device_attributes::NetworkInterface {
            interface_name: "eth1".to_string(),
            mac_address: usable_mac.to_string(),
            ip_address: None,
            network_id: None,
            assigned_network_id: Some(network_id),
            assigned_ip: Some("10.0.0.50".to_string()),
            speed_mbps: Some(1000),
            disabled: false,
            warning_label: None,
        };

        let (network_info, nics) =
            match build_network_contexts(&conn, &uuid, &[disabled_iface, usable_iface]).await {
                Ok(v) => v,
                Err(_) => panic!("Expected Ok from build_network_contexts"),
            };

        assert_eq!(nics.len(), 1, "disabled NIC must be excluded");
        assert_eq!(nics[0].mac_address, usable_mac);
        assert!(nics[0].is_primary);
        assert_eq!(network_info.mac_address, usable_mac);
        assert_eq!(network_info.ip_address, "10.0.0.50");
    }

    #[tokio::test]
    async fn test_render_for_device_no_role() {
        use crate::test_connection_factory;
        let (state, _temp_dir, _migration_conn) =
            create_test_state(test_connection_factory!()).await;

        let uuid = test_uuid();
        {
            let conn = state.connection_factory.open().await.unwrap();
            Director::new(&conn)
                .register_device(&uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let conn = state.connection_factory.open().await.unwrap();
        let result =
            render_for_device(&conn, &state.image_store, None, &uuid, "http://localhost").await;
        // Should get NotFound error when device has no role
        match result {
            Ok(_) => panic!("Expected error but got Ok"),
            Err(Error::NotFound(_)) => {
                // Success - this is the expected error
            }
            Err(Error::ServerInternalError(e)) => {
                panic!("Expected NotFound but got ServerInternalError: {}", e)
            }
            Err(Error::ValidationError(_)) => panic!("Expected NotFound but got ValidationError"),
            Err(Error::BadRequest(_)) => panic!("Expected NotFound but got BadRequest"),
            Err(Error::UnprocessableEntity(_)) => {
                panic!("Expected NotFound but got UnprocessableEntity")
            }
        }
    }
}
