use clap::Parser;
use rack_director::rack_director_start;

#[tokio::main]
async fn main() {
    // First step is to configure the logger.
    env_logger::init_from_env(env_logger::Env::default().filter_or("LOG", "info"));

    let args = rack_director::Args::parse();

    let start_result = rack_director_start(args)
        .await
        .expect("Error starting Rack Director");
    start_result.wait().await;
}
