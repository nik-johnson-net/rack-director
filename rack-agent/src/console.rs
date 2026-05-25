use std::time::Duration;

use anyhow::{Result, anyhow};
use common::cnc::CncClient;
use log::info;

use crate::scan;

// Start the console when the UUID is not known
pub async fn start_console_command(client: &CncClient) -> Result<()> {
    let uuid = scan::read_dmi_for_uuid()
        .await?
        .ok_or(anyhow!("uuid not found"))?;
    start_console(client, &uuid).await
}

// Start the console when the UUID can be provided. Saves a DMI table scan.
pub async fn start_console(client: &CncClient, uuid: &str) -> Result<()> {
    info!("Starting console");
    let mut process = tokio::process::Command::new("bash")
        .env("PS0", "#> ")
        .stdin(open_console_input()?)
        .stdout(open_console_output()?)
        .stderr(open_console_output()?)
        .spawn()
        .expect("failed to start bash");

    // Set success
    client.action_success(uuid).await?;

    // Wait for process to exit
    let exit_code = loop {
        match process.try_wait()? {
            None => tokio::time::sleep(Duration::from_secs(2)).await,
            Some(exit_code) => break exit_code,
        };
    };

    info!("User exited console (exit code {})", exit_code);
    Ok(())
}

fn open_console_input() -> Result<std::fs::File, std::io::Error> {
    std::fs::File::options().read(true).open("/dev/console")
}

fn open_console_output() -> Result<std::fs::File, std::io::Error> {
    std::fs::File::options().write(true).open("/dev/console")
}
