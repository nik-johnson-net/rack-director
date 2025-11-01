mod database;
mod dhcp;
mod director;
mod http;
mod lifecycle;
mod plans;
mod tftp;

use std::{io, sync::Arc};

use clap::Parser;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::director::Director;

#[cfg(debug_assertions)]
const DEFAULT_DATABASE_PATH: &str = ".db.sqlite";

#[cfg(not(debug_assertions))]
const DEFAULT_DATABASE_PATH: &str = "/var/lib/rack-director/db.sqlite";

#[derive(Parser, Debug)]
pub struct Args {
    // Path to the database file.
    #[arg(long, default_value_t = DEFAULT_DATABASE_PATH.to_string())]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = "/usr/lib/rack-director/tftp")]
    tftp_path: String,

    // DHCP server port (optional, defaults to 67)
    #[arg(long, default_value = "0.0.0.0:67")]
    dhcp_address: String,

    #[arg(long, default_value = "0.0.0.0:3000")]
    http_address: String,

    #[arg(long, default_value = "0.0.0.0:69")]
    tftp_address: String,
}

pub struct RackDirectorHandle {
    // Information for the http service
    http_handle: JoinHandle<Result<(), io::Error>>,
    pub http_port: u16,

    // Information for the tftp service
    tftp_handle: JoinHandle<Result<(), anyhow::Error>>,
    pub tftp_port: u16,

    // Information for the dhcp service
    dhcp_handle: JoinHandle<Result<(), anyhow::Error>>,
    pub dhcp_port: u16,
}

impl RackDirectorHandle {
    // Wait for the services to stop.
    // TODO: Currently just waiting for any one to abort. Need a proper abort signal / shutdown architecture.
    pub async fn wait(self) {
        let _ = tokio::try_join!(self.http_handle, self.tftp_handle, self.dhcp_handle);
    }
}

pub async fn rack_director_start(args: crate::Args) -> Result<RackDirectorHandle, anyhow::Error> {
    let db = Arc::new(Mutex::new(database::open(&args.db_path).unwrap()));
    let director: Director = Director::new(db.clone());
    let tftp_handler = director::DirectorTftpHandler::new(args.tftp_path);

    // Initialize DHCP server and store
    let dhcp_store = dhcp::DhcpStore::new(db.clone());
    let dhcp_server = dhcp::DhcpServer::new(db.clone(), director.clone(), Some(args.dhcp_address))
        .await
        .unwrap();

    let mut tftp_server = tftp::Server::new(tftp_handler);
    tftp_server.address(args.tftp_address);

    let http_start_result = http::start(director.clone(), dhcp_store, args.http_address).await?;
    let tftp_start_result = tftp_server.serve().await?;
    let dhcp_start_result = dhcp_server.serve().await?;

    Ok(RackDirectorHandle {
        http_handle: http_start_result.join_handle,
        http_port: http_start_result.port,

        tftp_handle: tftp_start_result.join_handle,
        tftp_port: tftp_start_result.port,

        dhcp_handle: dhcp_start_result.join_handle,
        dhcp_port: dhcp_start_result.port,
    })
}
