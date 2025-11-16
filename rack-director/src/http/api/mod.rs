// API module reserved for future external programmatic access
// Currently, all endpoints are under /ui/ for the web interface
// Future third-party integrations, CLI tools, or external consumers
// should use endpoints defined here under the /api/ prefix

use axum::Router;
use std::sync::Arc;

use crate::http::AppState;

pub fn routes(_state: Arc<AppState>) -> Router {
    Router::new()
    // No routes defined yet
}
