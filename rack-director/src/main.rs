use clap::Parser;
use rack_director::rack_director_start;

#[tokio::main]
async fn main() {
    // First step is to configure the logger.
    std_logger::Config::logfmt().init();

    let args = rack_director::Args::parse();

    let start_result = rack_director_start(args)
        .await
        .expect("Error starting Rack Director");
    start_result.wait().await;
}
