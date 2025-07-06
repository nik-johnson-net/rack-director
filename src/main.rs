mod database;
mod dhcp;
mod director;
mod http;
mod tftp;

use clap::Parser;
use tokio::sync::Mutex;
use std::net::Ipv4Addr;
use std::sync::Arc;

const DEFAULT_DATABASE_PATH: &str = "/var/lib/rack-director/db.sqlite";

#[derive(Parser, Debug)]
struct Args {
    // Path to the database file.
    #[arg(long, default_value_t = DEFAULT_DATABASE_PATH.to_string())]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = "/usr/lib/rack-director/tftp")]
    tftp_path: String,
    
    // IPv4 address for the DHCP server
    #[arg(long, default_value = "192.168.1.1")]
    dhcp_server_ipv4: String,
    
    // Enable DHCP server
    #[arg(long, default_value = "true")]
    enable_dhcp: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let db = Arc::new(Mutex::new(database::open(&args.db_path).unwrap()));

    let tftp_handler = director::DirectorTftpHandler::new(args.tftp_path);

    let http_handle = tokio::spawn(http::start(Arc::clone(&db)));
    let tftp_handle = tokio::spawn(tftp::Server::new(tftp_handler).serve());
    
    // Start DHCP server if enabled
    let dhcp_handle = if args.enable_dhcp {
        let dhcp_server_ip: Ipv4Addr = args.dhcp_server_ipv4.parse()
            .expect("Invalid DHCP server IPv4 address");
        let dhcp_server = dhcp::DhcpServer::new(Arc::clone(&db), dhcp_server_ip, None);
        
        Some(tokio::spawn(async move {
            if let Err(e) = dhcp_server.start().await {
                log::error!("DHCP server error: {}", e);
            }
        }))
    } else {
        None
    };

    http_handle.await.unwrap().unwrap();
    log::info!("http server shutdown");

    tftp_handle.await.unwrap().unwrap();
    log::info!("tftp server shutdown");
    
    if let Some(handle) = dhcp_handle {
        handle.await.unwrap();
        log::info!("dhcp server shutdown");
    }
}
