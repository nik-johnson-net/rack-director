mod api;
mod cnc;
mod error;
mod ui;

#[cfg(test)]
pub(crate) mod test_helpers;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::task::JoinHandle;

use crate::boot_files::BootFileProvider;
use crate::database::ConnectionFactory;
use crate::dhcp::DhcpControl;
use crate::storage::ImageStore;

/// Shared application state for all HTTP handlers.
///
/// Handlers open a fresh database connection per request via the `db` factory.
/// This avoids connection sharing across async tasks and ensures each
/// request has an independent SQLite connection.
pub struct AppState {
    pub connection_factory: Arc<dyn ConnectionFactory>,
    pub image_store: Arc<ImageStore>,
    pub agent_images_path: PathBuf,
    pub boot_file_provider: Arc<dyn BootFileProvider>,
    /// Handle to the DHCP socket manager, used by network create/delete
    /// handlers to bind or release per-network sockets in real time.
    pub dhcp: DhcpControl,
    /// Number of seconds an unprovisioned or unknown device sleeps before
    /// rebooting to retry PXE boot.  Configurable so e2e tests can set it to 0.
    pub unprovisioned_sleep_secs: u64,
    /// Path to the bundled Default OSM directory on disk.
    ///
    /// When set, requests for bundled OSM files are served directly from this
    /// directory rather than from the image store (which does not contain them).
    pub bundled_osm_path: Option<PathBuf>,
}

pub struct StartResult {
    pub join_handle: JoinHandle<Result<(), std::io::Error>>,
    pub port: u16,
}

pub async fn start<T: Into<SocketAddr>>(
    connection_factory: Arc<dyn ConnectionFactory>,
    image_store: Arc<ImageStore>,
    bind: T,
    agent_images_path: PathBuf,
    boot_file_provider: Arc<dyn BootFileProvider>,
    dhcp: DhcpControl,
    unprovisioned_sleep_secs: u64,
    bundled_osm_path: Option<PathBuf>,
) -> Result<StartResult> {
    let state = Arc::new(AppState {
        connection_factory,
        image_store,
        agent_images_path,
        boot_file_provider,
        dhcp,
        unprovisioned_sleep_secs,
        bundled_osm_path,
    });

    let app = Router::new()
        .merge(ui::routes(state.clone()))
        .merge(cnc::routes(state.clone()))
        .merge(api::routes(state));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(bind.into()).await?;
    let local_addr = listener.local_addr().expect("local_addr");

    log::info!("Starting http server on {}", local_addr);
    let join_handle = tokio::spawn(
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    Ok(StartResult {
        join_handle,
        port: local_addr.port(),
    })
}
