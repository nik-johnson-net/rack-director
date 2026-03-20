mod devices;
mod platforms;

use axum::Router;
use std::sync::Arc;

use crate::http::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(devices::routes(state.clone()))
        .merge(platforms::routes(state))
}
