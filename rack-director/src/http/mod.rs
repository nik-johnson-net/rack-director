mod api;
mod cnc;
mod error;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::task::JoinHandle;

use crate::dhcp::DhcpStore;
use crate::director::Director;

struct AppState {
    director: Director,
    dhcp_store: DhcpStore,
}

pub struct StartResult {
    pub join_handle: JoinHandle<Result<(), std::io::Error>>,
    pub port: u16,
}

pub async fn start<T: AsRef<str>>(
    director: Director,
    dhcp_store: DhcpStore,
    bind: T,
) -> Result<StartResult> {
    let state = Arc::new(AppState {
        director,
        dhcp_store,
    });

    let app = Router::new()
        .merge(ui::routes(state.clone()))
        .merge(cnc::routes(state.clone()))
        .merge(api::routes(state.clone()));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(bind.as_ref()).await?;
    let local_addr = listener.local_addr().expect("local_addr");

    log::info!("Starting http server on {}", local_addr);
    let join_handle = tokio::spawn(axum::serve(listener, app).into_future());
    Ok(StartResult {
        join_handle,
        port: local_addr.port(),
    })
}
