use super::{AppState, error::Error as HttpError};
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
        .route("/api/roles", post(create_role))
        .route("/api/roles", get(list_roles))
        .route("/api/roles/{id}", get(get_role))
        .route("/api/roles/{id}", put(update_role))
        .route("/api/roles/{id}", delete(delete_role))
        .route("/api/roles/{id}/devices", get(list_role_devices))
        .route("/api/devices/{uuid}/role", post(assign_role))
        .route("/api/devices/{uuid}/role", get(get_device_role))
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
    let devices = state.roles_store.list_devices_with_role(role_id).await?;
    Ok(Json(devices))
}

// Assign a role to a device
async fn assign_role(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
    Json(req): Json<AssignRoleRequest>,
) -> Result<StatusCode, HttpError> {
    // Verify role exists
    state.roles_store.get(req.role_id).await?;

    // Verify device exists
    state.director.get_device(&uuid).await?;

    state
        .roles_store
        .assign_to_device(&uuid, req.role_id)
        .await?;

    Ok(StatusCode::OK)
}

// Get the role assigned to a device
async fn get_device_role(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<String>,
) -> Result<Json<Option<Role>>, HttpError> {
    let role = state.roles_store.get_device_role(&uuid).await?;
    Ok(Json(role))
}
