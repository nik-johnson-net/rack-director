use anyhow::Result;
use anyhow::anyhow;
use clap::Subcommand;
use clap::{Parser, arg};
use log::error;
use log::info;

mod client;
mod scan;

#[derive(Subcommand, Debug)]
enum Command {
    // Scans the device and uploads metadata to Rack Director.
    DeviceScan(scan::DeviceScanArgs),
}

#[derive(Parser, Debug)]
struct Args {
    // URL to the Rack Director API. Uses /proc/cmdline if not provided.
    #[arg(long, help = "URL to the Rack Director API")]
    director_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() {
    // First step is to configure the logger.
    std_logger::Config::logfmt().init();

    info!("Starting Rack Agent...");
    let args = Args::parse();
    let director_url = resolve_director_url(args.director_url)
        .await
        .unwrap_or_else(|e| {
            log::error!("Failed to resolve director URL: {e}");
            std::process::exit(1);
        });

    let client = client::RackDirector::new(&director_url);

    let result = match args.command {
        Command::DeviceScan(device_args) => scan::device_scan(&client, &device_args).await,
    };

    if let Err(e) = result {
        error!("{e}");
        std::process::exit(10);
    }
}

// Locate the director URL
async fn resolve_director_url(arg_director_url: Option<String>) -> Result<String> {
    if let Some(url) = arg_director_url {
        Ok(url)
    } else {
        // Fallback to /proc/cmdline
        let cmdline = tokio::fs::read_to_string("/proc/cmdline").await?;
        let url = cmdline
            .split_whitespace()
            .find(|s| s.starts_with("rackdirector.url="))
            .and_then(|s| s.strip_prefix("rackdirector.url="));
        Ok(url
            .ok_or(anyhow!("Failed to find rackdirector.url in /proc/cmdline. Try giving it as flag --director-url"))?
            .to_string())
    }
}
