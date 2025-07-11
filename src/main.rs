mod database;
mod director;
mod http;
mod tftp;

use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;

use crate::director::Director;

const DEFAULT_DATABASE_PATH: &str = "/var/lib/rack-director/db.sqlite";

#[derive(Parser, Debug)]
struct Args {
    // Path to the database file.
    #[arg(long, default_value_t = DEFAULT_DATABASE_PATH.to_string())]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = "/usr/lib/rack-director/tftp")]
    tftp_path: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let db = Arc::new(Mutex::new(database::open(&args.db_path).unwrap()));
    let director: Director = Director::new(db.clone());
    let tftp_handler = director::DirectorTftpHandler::new(args.tftp_path);

    let http_handle = tokio::spawn(http::start(director.clone()));
    let tftp_handle = tokio::spawn(tftp::Server::new(tftp_handler).serve());

    http_handle.await.unwrap().unwrap();
    log::info!("http server shutdown");

    tftp_handle.await.unwrap().unwrap();
    log::info!("tftp server shutdown");
}
