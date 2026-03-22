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
    // Validate the disk layout before persisting.
    if let Err(errors) = crate::disk_layout::validate_disk_layout(&req.disk_layout) {
        return Err(HttpError::ValidationError(errors));
    }

    let conn = state.connection_factory.open().await?;

    // Verify the OS exists
    crate::operating_systems::store::get(&conn, req.os_id).await?;

    let role = crate::roles::store::create(
        &conn,
        &req.name,
        req.description.as_deref(),
        req.os_id,
        &req.disk_layout,
        req.config_template.as_ref(),
        req.firmware_mode,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(role)))
}

// List all roles
async fn list_roles(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RoleWithOs>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let roles = crate::roles::store::list_with_os(&conn).await?;
    Ok(Json(roles))
}

// Get a specific role
async fn get_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<RoleWithOs>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let role = crate::roles::store::get_with_os(&conn, id).await?;
    Ok(Json(role))
}

// Update a role
#[axum::debug_handler]
async fn update_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<Role>, HttpError> {
    // Validate the new disk layout before applying any changes.
    if let Some(ref disk_layout) = req.disk_layout
        && let Err(errors) = crate::disk_layout::validate_disk_layout(disk_layout)
    {
        return Err(HttpError::ValidationError(errors));
    }

    let conn = state.connection_factory.open().await?;

    // If updating OS, verify it exists
    if let Some(os_id) = req.os_id {
        crate::operating_systems::store::get(&conn, os_id).await?;
    }

    // Platform compatibility check: if the new disk layout uses labels, verify that every
    // device currently assigned to this role has a platform that satisfies all required labels.
    if let Some(ref new_layout) = req.disk_layout
        && crate::disk_layout::layout_uses_labels(new_layout)
    {
        check_platform_compatibility(&conn, id, new_layout).await?;
    }

    let role = crate::roles::store::update(
        &conn,
        id,
        crate::roles::store::UpdateRoleParams {
            name: req.name.as_deref(),
            description: req.description.as_deref(),
            os_id: req.os_id,
            disk_layout: req.disk_layout.as_ref(),
            config_template: req.config_template.as_ref(),
            firmware_mode: req.firmware_mode,
            clear_firmware_mode: req.clear_firmware_mode,
        },
    )
    .await?;

    Ok(Json(role))
}

/// Check that every device assigned to `role_id` has a platform that satisfies all disk
/// labels in `new_layout`.
///
/// Returns an `HttpError::ValidationError` if any device's platform cannot resolve all labels,
/// so the caller receives a structured 400 response instead of an internal server error.
///
/// Devices without a platform are skipped — label resolution for those will fail at
/// provisioning time, which is handled elsewhere.
async fn check_platform_compatibility(
    conn: &crate::database::Connection,
    role_id: i64,
    new_layout: &crate::roles::DiskLayout,
) -> Result<(), HttpError> {
    let device_uuids = crate::director::store::list_devices_with_role(conn, role_id).await?;

    // Collect distinct platform IDs without loading full device rows.
    let mut platform_ids = std::collections::HashSet::new();
    for uuid in &device_uuids {
        if let Some(pid) = crate::director::store::get_device_platform_id(conn, uuid).await? {
            platform_ids.insert(pid);
        }
    }

    // Validate each distinct platform once, avoiding redundant queries when many devices
    // share the same platform.
    for platform_id in &platform_ids {
        let platform = crate::platforms::store::get(conn, *platform_id).await?;

        if let Err(e) =
            crate::disk_layout::validate_layout_against_platform(new_layout, &platform.attributes)
        {
            let mut errors = std::collections::HashMap::new();
            errors.insert(
                "disk_layout".to_string(),
                format!(
                    "Platform '{}' does not have all required disk labels: {}. \
                     Remove the role from affected devices first, or update the platform.",
                    platform.name, e
                ),
            );
            return Err(HttpError::ValidationError(errors));
        }
    }

    Ok(())
}

// Delete a role
async fn delete_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    crate::roles::store::delete(&conn, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// List all devices with a specific role
async fn list_role_devices(
    State(state): State<Arc<AppState>>,
    Path(role_id): Path<i64>,
) -> Result<Json<Vec<String>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let devices = crate::director::store::list_devices_with_role(&conn, role_id).await?;
    let device_strs: Vec<String> = devices.iter().map(|u| u.to_string()).collect();
    Ok(Json(device_strs))
}
