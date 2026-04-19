mod build;
mod validate;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rack-director-osm", about = "Build and validate OSM packages")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate an OSM archive file
    Validate {
        /// Path to the OSM archive (.tar.zst)
        file: std::path::PathBuf,
    },
    /// Build an OSM from the current directory
    Build,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate { file } => validate::run(&file),
        Commands::Build => build::run().await,
    }
}
