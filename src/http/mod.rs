mod cnc;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::sync::Mutex;

struct AppState {
    db: Arc<Mutex<rusqlite::Connection>>,
}

pub async fn start(db: Arc<Mutex<rusqlite::Connection>>) -> Result<()> {
    let state = Arc::new(AppState { db });

    let app = Router::new()
        .merge(ui::routes(state.clone()))
        .merge(cnc::routes(state.clone()));

    log::info!("Starting http server on 0.0.0.0:3000");
    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
