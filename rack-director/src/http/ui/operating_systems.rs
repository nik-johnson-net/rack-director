use crate::operating_systems::{
    Architecture, OperatingSystem, OsArchitecture, store::OperatingSystemWithArchitectures,
};

use super::super::{AppState, error::Error as HttpError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use bytes::Bytes;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Request to create a new operating system
#[derive(Debug, Deserialize)]
pub struct CreateOperatingSystemRequest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

/// Request to update an operating system
#[derive(Debug, Deserialize)]
pub struct UpdateOperatingSystemRequest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// Request to create or update an OS architecture configuration
#[derive(Debug, Deserialize)]
pub struct CreateOsArchitectureRequest {
    pub architecture: Architecture,
    pub kernel_path: Option<String>,
    pub initramfs_path: Option<String>,
    pub modules: Option<Vec<String>>,
    pub cmdline_args: Option<String>,
    pub install_script_path: Option<String>,
}

/// Request to update an OS architecture configuration
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields are read by Serde during deserialization
pub struct UpdateOsArchitectureRequest {
    pub kernel_path: Option<String>,
    pub initramfs_path: Option<String>,
    pub modules: Option<Vec<String>>,
    pub cmdline_args: Option<String>,
    pub install_script_path: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/operating_systems", post(create_os))
        .route("/ui/operating_systems", get(list_os))
        .route("/ui/operating_systems/{id}", get(get_os))
        .route("/ui/operating_systems/{id}", put(update_os))
        .route("/ui/operating_systems/{id}", delete(delete_os))
        .route(
            "/ui/operating_systems/{id}/architectures",
            post(create_os_architecture),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}",
            get(get_os_architecture),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}",
            delete(delete_os_architecture),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}/kernel",
            post(upload_kernel),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}/initramfs",
            post(upload_initramfs),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}/modules",
            post(upload_module),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}/install_script",
            post(upload_install_script),
        )
        .route(
            "/ui/operating_systems/{id}/architectures/{arch}/download/{component}",
            get(download_component),
        )
        .with_state(state)
}

// Create a new operating system
async fn create_os(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateOperatingSystemRequest>,
) -> Result<(StatusCode, Json<OperatingSystem>), HttpError> {
    let os = state
        .os_store
        .create(&req.name, &req.version, req.description.as_deref())
        .await?;

    Ok((StatusCode::CREATED, Json(os)))
}

// List all operating systems
async fn list_os(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<OperatingSystem>>, HttpError> {
    let systems = state.os_store.list().await?;
    Ok(Json(systems))
}

// Get a specific operating system with all architectures
async fn get_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<OperatingSystemWithArchitectures>, HttpError> {
    let os = state.os_store.get_with_architectures(id).await?;
    Ok(Json(os))
}

// Update an operating system
#[axum::debug_handler]
async fn update_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateOperatingSystemRequest>,
) -> Result<Json<OperatingSystem>, HttpError> {
    let os = state
        .os_store
        .update(
            id,
            req.name.as_deref(),
            req.version.as_deref(),
            req.description.as_deref(),
        )
        .await?;

    Ok(Json(os))
}

// Delete an operating system
async fn delete_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.os_store.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// Create or update an OS architecture configuration
async fn create_os_architecture(
    State(state): State<Arc<AppState>>,
    Path(os_id): Path<i64>,
    Json(req): Json<CreateOsArchitectureRequest>,
) -> Result<(StatusCode, Json<OsArchitecture>), HttpError> {
    // Generate placeholder paths (will be updated when files are uploaded)
    let kernel_path = req
        .kernel_path
        .unwrap_or_else(|| format!("os/{}/arch/{}/kernel", os_id, req.architecture.as_str()));
    let initramfs_path = req
        .initramfs_path
        .unwrap_or_else(|| format!("os/{}/arch/{}/initramfs", os_id, req.architecture.as_str()));

    let arch = state
        .os_store
        .upsert_architecture(
            os_id,
            req.architecture,
            &kernel_path,
            &initramfs_path,
            req.modules.unwrap_or_default(),
            req.cmdline_args.as_deref(),
            req.install_script_path.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(arch)))
}

// Get a specific architecture configuration
async fn get_os_architecture(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
) -> Result<Json<OsArchitecture>, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let os_arch = state.os_store.get_architecture(os_id, arch).await?;
    Ok(Json(os_arch))
}

// Delete an architecture configuration
async fn delete_os_architecture(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
) -> Result<StatusCode, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    state.os_store.delete_architecture(os_id, arch).await?;
    Ok(StatusCode::NO_CONTENT)
}

// Upload kernel
async fn upload_kernel(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<OsArchitecture>, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let path = format!("os/{}/arch/{}/kernel", os_id, arch.as_str());
    let filename = params.get("filename").map(|s| s.as_str());

    state.image_store.upload(&path, body.to_vec()).await?;
    state
        .os_store
        .update_architecture_field(os_id, arch, "kernel_path", &path)
        .await?;

    if let Some(filename) = filename {
        state
            .os_store
            .update_architecture_field(os_id, arch, "kernel_filename", filename)
            .await?;
    }

    let os_arch = state.os_store.get_architecture(os_id, arch).await?;
    Ok(Json(os_arch))
}

// Upload initramfs
async fn upload_initramfs(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<OsArchitecture>, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let path = format!("os/{}/arch/{}/initramfs", os_id, arch.as_str());
    let filename = params.get("filename").map(|s| s.as_str());

    state.image_store.upload(&path, body.to_vec()).await?;
    state
        .os_store
        .update_architecture_field(os_id, arch, "initramfs_path", &path)
        .await?;

    if let Some(filename) = filename {
        state
            .os_store
            .update_architecture_field(os_id, arch, "initramfs_filename", filename)
            .await?;
    }

    let os_arch = state.os_store.get_architecture(os_id, arch).await?;
    Ok(Json(os_arch))
}

// Upload module
async fn upload_module(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<OsArchitecture>, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let module_name = params
        .get("name")
        .ok_or_else(|| HttpError::BadRequest("Missing 'name' query parameter".into()))?;

    let path = format!(
        "os/{}/arch/{}/modules/{}",
        os_id,
        arch.as_str(),
        module_name
    );

    state.image_store.upload(&path, body.to_vec()).await?;

    // Add module to the architecture's modules list
    let mut os_arch = state.os_store.get_architecture(os_id, arch).await?;
    if !os_arch.modules.contains(&path) {
        os_arch.modules.push(path.clone());
        let modules_json = serde_json::to_string(&os_arch.modules)?;
        state
            .os_store
            .update_architecture_field(os_id, arch, "modules", &modules_json)
            .await?;
    }

    let os_arch = state.os_store.get_architecture(os_id, arch).await?;
    Ok(Json(os_arch))
}

// Upload install script
async fn upload_install_script(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str)): Path<(i64, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Json<OsArchitecture>, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let path = format!("os/{}/arch/{}/install_script", os_id, arch.as_str());
    let filename = params.get("filename").map(|s| s.as_str());

    state.image_store.upload(&path, body.to_vec()).await?;
    state
        .os_store
        .update_architecture_field(os_id, arch, "install_script_path", &path)
        .await?;

    if let Some(filename) = filename {
        state
            .os_store
            .update_architecture_field(os_id, arch, "install_script_filename", filename)
            .await?;
    }

    let os_arch = state.os_store.get_architecture(os_id, arch).await?;
    Ok(Json(os_arch))
}

// Download a component (kernel, initramfs, module, install_script)
async fn download_component(
    State(state): State<Arc<AppState>>,
    Path((os_id, arch_str, component)): Path<(i64, String, String)>,
) -> Result<impl IntoResponse, HttpError> {
    let arch = Architecture::from_str(&arch_str)?;
    let os_arch = state.os_store.get_architecture(os_id, arch).await?;

    let path = match component.as_str() {
        "kernel" => os_arch.kernel_path,
        "initramfs" => os_arch.initramfs_path,
        "install_script" => os_arch
            .install_script_path
            .ok_or_else(|| HttpError::NotFound("Install script not found".into()))?,
        _ => {
            // Check if it's a module
            if let Some(module_path) = os_arch
                .modules
                .iter()
                .find(|m| m.ends_with(&format!("/{}", component)))
            {
                module_path.clone()
            } else {
                return Err(HttpError::NotFound(format!(
                    "Component not found: {}",
                    component
                )));
            }
        }
    };

    let data = state.image_store.download(&path).await?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/octet-stream")],
        data,
    ))
}
