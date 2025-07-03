use std::sync::Arc;

use axum::{Router, extract::State, routing::get};

use crate::http::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(http_index))
        .with_state(state)
}

async fn http_index(State(_state): State<Arc<AppState>>) -> String {
    "Hello, world!".to_string()
}
