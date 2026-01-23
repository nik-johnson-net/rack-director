mod devices;
mod dhcp;
mod networks;
mod operating_systems;
mod roles;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};

use crate::http::AppState;

const DEFAULT_UI_PATH: &str = env!("RACK_DIRECTOR_UI_PATH");

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        // Static asset serving
        .route("/assets/{asset}", get(http_assets))
        .route("/", get(http_index))
        .route("/{*wildcard}", get(http_index))
        .with_state(state.clone())
        // Merge all UI API routes
        .merge(devices::routes(state.clone()))
        .merge(dhcp::routes(state.clone()))
        .merge(networks::routes(state.clone()))
        .merge(operating_systems::routes(state.clone()))
        .merge(roles::routes(state))
}

async fn http_index() -> Result<Html<Vec<u8>>, StatusCode> {
    let path = PathBuf::from(DEFAULT_UI_PATH).join("index.html");
    match tokio::fs::read(path).await {
        Ok(data) => Ok(Html(data)),
        Err(e) => {
            log::error!("index not found: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

async fn http_assets(Path(asset): Path<String>) -> impl IntoResponse {
    let canonicalized_root = tokio::fs::canonicalize(DEFAULT_UI_PATH).await.unwrap();
    let path = PathBuf::from(DEFAULT_UI_PATH).join("assets").join(&asset);
    let canonicalized_path = tokio::fs::canonicalize(path).await.unwrap();
    if !canonicalized_path.starts_with(canonicalized_root) {
        log::warn!(
            "Asset requested for invalid path: {} ({})",
            asset,
            canonicalized_path.to_string_lossy()
        );
        return Err(StatusCode::NOT_FOUND);
    }

    match tokio::fs::read(canonicalized_path).await {
        Ok(data) => {
            let content_type = match asset.rsplit_once('.') {
                Some((_, "js")) => "text/javascript",
                Some((_, "css")) => "text/css",
                Some((_, _)) => "text/plain",
                None => "text/plain",
            };
            Ok(([(header::CONTENT_TYPE, content_type)], data))
        }
        Err(e) => {
            log::warn!("Asset not found: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}
