use clap::Parser;
use rack_director::RackDirectorHandle;
use serde_json::json;
use tempfile::TempDir;

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
