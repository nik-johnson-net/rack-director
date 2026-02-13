use super::super::{AppState, error::Error as HttpError};
use crate::roles::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/roles", post(create_role))
        .route("/ui/roles", get(list_roles))
        .route("/ui/roles/{id}", get(get_role))
        .route("/ui/roles/{id}", put(update_role))
        .route("/ui/roles/{id}", delete(delete_role))
        .route("/ui/roles/{id}/devices", get(list_role_devices))
        .with_state(state)
}

// Create a new role
async fn create_role(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<Role>), HttpError> {
    // Verify the OS exists
    state.os_store.get(req.os_id).await?;

    let role = state
        .roles_store
        .create(
            &req.name,
            req.description.as_deref(),
            req.os_id,
            &req.disk_layout,
            req.config_template.as_ref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(role)))
}

// List all roles
async fn list_roles(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RoleWithOs>>, HttpError> {
    let roles = state.roles_store.list_with_os().await?;
    Ok(Json(roles))
}

// Get a specific role
async fn get_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<RoleWithOs>, HttpError> {
    let role = state.roles_store.get_with_os(id).await?;
    Ok(Json(role))
}

// Update a role
#[axum::debug_handler]
async fn update_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<Role>, HttpError> {
    // If updating OS, verify it exists
    if let Some(os_id) = req.os_id {
        state.os_store.get(os_id).await?;
    }

    let role = state
        .roles_store
        .update(
            id,
            req.name.as_deref(),
            req.description.as_deref(),
            req.os_id,
            req.disk_layout.as_ref(),
            req.config_template.as_ref(),
        )
        .await?;

    Ok(Json(role))
}

// Delete a role
async fn delete_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.roles_store.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// List all devices with a specific role
async fn list_role_devices(
    State(state): State<Arc<AppState>>,
    Path(role_id): Path<i64>,
) -> Result<Json<Vec<String>>, HttpError> {
    let devices = state.director.list_devices_with_role(role_id).await?;
    let device_strs: Vec<String> = devices.iter().map(|u| u.to_string()).collect();
    Ok(Json(device_strs))
}
