use clap::Parser;
use rack_director::RackDirectorHandle;

pub mod dhcp_client;
pub mod tftp_client;

pub async fn start_rack_director() -> Result<RackDirectorHandle, anyhow::Error> {
    // Create a temporary directory for database
    let db_dir = tempfile::tempdir()?;
    let db_path = db_dir.path().to_str().unwrap().to_string();

    // Keep the temp directory alive by leaking it (test will clean up on exit)
    std::mem::forget(db_dir);

    // Create a temporary directory for image storage
    let storage_dir = tempfile::tempdir()?;
    let storage_path = storage_dir.path().to_str().unwrap().to_string();

    // Keep the temp directory alive by leaking it (test will clean up on exit)
    std::mem::forget(storage_dir);

    // Get absolute path to TFTP fixtures
    let tftp_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tftp");

    let args = rack_director::Args::parse_from([
        "test",
        &format!("--db-path={}", db_path),
        &format!("--tftp-path={}", tftp_path.display()),
        &format!("--storage-path={}", storage_path),
        "--dhcp-address=127.0.0.1:0",
        "--http-address=127.0.0.1:0",
        "--tftp-address=127.0.0.1:0",
        "--tftp-public-address=10.0.0.1",
        "--http-public-url=http://10.0.0.1",
        "--enable-autodiscover",
    ]);
    let handle = rack_director::rack_director_start(args).await?;

    // Give services a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(handle)
}
