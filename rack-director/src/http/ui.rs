use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Serialize;

use crate::http::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/assets/{asset}", get(http_assets))
        .route("/ui/devices", get(devices_index))
        .route("/", get(http_index))
        .route("/{*wildcard}", get(http_index))
        .with_state(state)
}

async fn http_index() -> Result<Html<Vec<u8>>, StatusCode> {
    match tokio::fs::read("./rack-director-ui/dist/index.html").await {
        Ok(data) => Ok(Html(data)),
        Err(e) => {
            log::error!("index not found: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

async fn http_assets(Path(asset): Path<String>) -> impl IntoResponse {
    match tokio::fs::read(format!("./rack-director-ui/dist/assets/{}", asset)).await {
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

#[derive(Serialize)]
struct Plan {
    id: u64,
    status: String,
    current_step: u32,
    total_steps: u32,
    error: String,
}

#[derive(Serialize)]
struct Device {
    uuid: String,
    hostname: String,
    plan: Option<Plan>,
}

#[derive(Serialize)]
struct DevicesIndex {
    devices: Vec<Device>,
}

async fn devices_index(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DevicesIndex>, StatusCode> {
    match state.director.get_all_devices().await {
        Ok(devices_data) => {
            let mut devices = Vec::new();

            for device in devices_data {
                let hostname = device
                    .attributes
                    .get("hostname")
                    .and_then(|h| h.as_str())
                    .unwrap_or(&device.uuid)
                    .to_string();

                let plan = match state
                    .director
                    .get_active_plan_for_device(&device.uuid)
                    .await
                {
                    Ok(Some(plan)) => Some(Plan {
                        id: plan.id.unwrap_or(0) as u64,
                        status: format!("{:?}", plan.status),
                        current_step: plan.current_step as u32,
                        total_steps: plan.actions.len() as u32,
                        error: plan.error_message.unwrap_or_default(),
                    }),
                    Ok(None) => None,
                    Err(_) => None,
                };

                devices.push(Device {
                    uuid: device.uuid,
                    hostname,
                    plan,
                });
            }

            let data = DevicesIndex { devices };
            Ok(Json(data))
        }
        Err(e) => {
            log::error!("Failed to fetch devices: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
