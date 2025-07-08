mod cnc;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;

use crate::director::Director;

struct AppState {
    director: Director,
}

pub async fn start(director: Director) -> Result<()> {
    let state = Arc::new(AppState { director });

    let app = Router::new()
        .merge(ui::routes(state.clone()))
        .merge(cnc::routes(state.clone()));

    log::info!("Starting http server on 0.0.0.0:3000");
    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
