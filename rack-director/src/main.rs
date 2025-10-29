mod database;
mod dhcp;
mod director;
mod http;
mod lifecycle;
mod plans;
mod tftp;

use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;

use crate::director::Director;

#[cfg(debug_assertions)]
const DEFAULT_DATABASE_PATH: &str = ".db.sqlite";

#[cfg(not(debug_assertions))]
const DEFAULT_DATABASE_PATH: &str = "/var/lib/rack-director/db.sqlite";

#[derive(Parser, Debug)]
struct Args {
    // Path to the database file.
    #[arg(long, default_value_t = DEFAULT_DATABASE_PATH.to_string())]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = "/usr/lib/rack-director/tftp")]
    tftp_path: String,

    // DHCP server port (optional, defaults to 67)
    #[arg(long)]
    dhcp_port: Option<u16>,
}

#[tokio::main]
async fn main() {
    // First step is to configure the logger.
    std_logger::Config::logfmt().init();

    let args = Args::parse();

    let db = Arc::new(Mutex::new(database::open(&args.db_path).unwrap()));
    let director: Director = Director::new(db.clone());
    let tftp_handler = director::DirectorTftpHandler::new(args.tftp_path);

    // Initialize DHCP server and store
    let dhcp_store = dhcp::DhcpStore::new(db.clone());
    let dhcp_server = dhcp::DhcpServer::new(db.clone(), director.clone(), args.dhcp_port)
        .await
        .unwrap();

    let http_handle = tokio::spawn(http::start(director.clone(), dhcp_store));
    let tftp_handle = tokio::spawn(tftp::Server::new(tftp_handler).serve());
    let dhcp_handle = tokio::spawn(dhcp_server.serve());

    tokio::select! {
        result = http_handle => {
            result.unwrap().unwrap();
            log::info!("http server shutdown");
        }
        result = tftp_handle => {
            result.unwrap().unwrap();
            log::info!("tftp server shutdown");
        }
        result = dhcp_handle => {
            result.unwrap().unwrap();
            log::info!("dhcp server shutdown");
        }
    }
}
