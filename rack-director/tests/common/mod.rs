use clap::Parser;
use rack_director::RackDirectorHandle;
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

pub mod dhcp_client;
pub mod tftp_client;

pub struct TestRackDirectorHandle {
    pub handle: RackDirectorHandle,

    // Handles to clean up tempdirs.
    // TODO: Should let rackdirector stop before dropping dirs
    _db_dir: TempDir,
    _storage_dir: TempDir,
}

impl TestRackDirectorHandle {
    pub async fn set_network_autodiscover(
        &self,
        network_id: u16,
        autodiscover: bool,
    ) -> Result<(), anyhow::Error> {
        let client = reqwest::Client::new();
        client
            .put(format!(
                "http://127.0.0.1:{}/ui/dhcp/networks/{}",
                self.handle.http_port, network_id
            ))
            .json(&json!({
                "enable_autodiscovery": autodiscover,
            }
            ))
            .send()
            .await?
            .error_for_status_ref()?;

        Ok(())
    }
}

/// Creates a test DHCP network via the HTTP API.
/// Returns the network ID.
pub async fn create_test_network(http_port: u16) -> Result<u64, anyhow::Error> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{}/ui/dhcp/networks", http_port))
        .json(&json!({
            "name": "Test",
            "subnet": "127.0.0.0/8",
            "gateway": "127.0.0.1",
            "dns_servers": ["8.8.8.8"],
            "lease_duration": 3600,
            "enable_autodiscovery": false
        }))
        .send()
        .await?
        .error_for_status()?;
    let network: serde_json::Value = response.json().await?;
    Ok(network["id"].as_u64().unwrap())
}

/// Creates a test DHCP pool for a given network via the HTTP API.
pub async fn create_test_pool(http_port: u16, network_id: u64) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::new();
    client
        .post(format!(
            "http://127.0.0.1:{}/ui/dhcp/networks/{}/pools",
            http_port, network_id
        ))
        .json(&json!({
            "name": "Test Pool",
            "range_start": "127.0.0.100",
            "range_end": "127.0.0.200"
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Register a test device via the standard PXE boot flow.
///
/// This performs the two-DHCP-exchange dance required to get a DHCP lease
/// with `device_uuid` set (needed for install_script rendering):
/// 1. DHCP exchange #1 → creates lease without device_uuid
/// 2. `GET /cnc/ipxe?uuid=<uuid>&mac=<mac>` → registers device, stores MAC
/// 3. DHCP exchange #2 → lease gets device_uuid linked
///
/// Returns the IP address assigned during the second DHCP exchange.
#[allow(dead_code)]
pub async fn register_test_device(
    http_port: u16,
    dhcp_port: u16,
    mac: [u8; 6],
    uuid: Uuid,
) -> Result<String, anyhow::Error> {
    use dhcp_client::{Architecture, DhcpClient};

    // DHCP exchange #1: creates a lease without device_uuid
    tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        let mut client = DhcpClient::new(mac, Architecture::X86Bios, dhcp_port)?;
        let (offered_ip, server_id) = client.discover()?;
        client.request(offered_ip, server_id)?;
        Ok(())
    })
    .await??;

    // HTTP call: registers the device and links MAC to UUID.
    // /cnc/ipxe always returns 200 with an iPXE script; error_for_status catches
    // any unexpected server-side failure during registration.
    let mac_str = format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/cnc/ipxe?uuid={}&mac={}",
            http_port, uuid, mac_str
        ))
        .send()
        .await?
        .error_for_status()?;

    // DHCP exchange #2: creates/updates the lease with device_uuid set
    let (leased_ip, _boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_, anyhow::Error> {
            let mut client = DhcpClient::new(mac, Architecture::X86Bios, dhcp_port)?;
            let (offered_ip, server_id) = client.discover()?;
            let result = client.request(offered_ip, server_id)?;
            Ok(result)
        })
        .await??;

    Ok(leased_ip.to_string())
}

pub async fn start_rack_director() -> Result<TestRackDirectorHandle, anyhow::Error> {
    // Initialize Logger for tests. Will be called multiple times, so throw away the result.
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();

    // Create a temporary directory for database
    let db_dir = tempfile::tempdir()?;
    let db_path = db_dir.path().to_str().unwrap().to_string();

    // Create a temporary directory for image storage
    let storage_dir = tempfile::tempdir()?;
    let storage_path = storage_dir.path().to_str().unwrap().to_string();

    // Get absolute path to firmware fixtures (shared by TFTP and HTTP)
    let tftp_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/firmware");

    let args = rack_director::Args::parse_from([
        "test",
        &format!("--db-path={}", db_path),
        &format!("--tftp-path={}", tftp_path.display()),
        &format!("--storage-path={}", storage_path),
        "--dhcp-address=0.0.0.0:0",
        "--dhcp-server-identifier=127.0.0.1",
        "--no-dhcp-broadcast",
        "--http-address=127.0.0.1:0",
        "--tftp-address=127.0.0.1:0",
        "--tftp-public-address=10.0.0.1",
        "--http-public-url=http://10.0.0.1",
    ]);
    let handle = rack_director::rack_director_start(args).await?;

    // Give services a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(TestRackDirectorHandle {
        handle,
        _db_dir: db_dir,
        _storage_dir: storage_dir,
    })
}
