use clap::Parser;
use rack_director::RackDirectorHandle;

pub async fn start_rack_director() -> Result<RackDirectorHandle, anyhow::Error> {
    let args = rack_director::Args::parse_from([
        "--db-path=.test.db.sqlite",
        "--tftp-path=./",
        "--dhcp-address=127.0.0.1:0",
        "--http-address=127.0.0.1:0",
        "--tftp-address=127.0.0.1:0",
    ]);
    let handle = rack_director::rack_director_start(args).await?;
    Ok(handle)
}
