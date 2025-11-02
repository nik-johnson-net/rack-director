use clap::Parser;
use rack_director::RackDirectorHandle;
use tempfile::NamedTempFile;

pub mod dhcp_client;
pub mod tftp_client;

pub async fn start_rack_director() -> Result<RackDirectorHandle, anyhow::Error> {
    // Create a temporary database file for this test run
    let db_file = NamedTempFile::new()?;
    let db_path = db_file.path().to_str().unwrap().to_string();

    // Keep the tempfile alive by leaking it (test will clean up on exit)
    std::mem::forget(db_file);

    // Get absolute path to TFTP fixtures
    let tftp_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tftp");

    let args = rack_director::Args::parse_from([
        "test",
        &format!("--db-path={}", db_path),
        &format!("--tftp-path={}", tftp_path.display()),
        "--dhcp-address=127.0.0.1:0",
        "--http-address=127.0.0.1:0",
        "--tftp-address=127.0.0.1:0",
    ]);
    let handle = rack_director::rack_director_start(args).await?;

    // Give services a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(handle)
}
