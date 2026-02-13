use super::super::{AppState, error::Error as HttpError};
use super::validation::*;
use crate::platforms::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/platforms", post(create_platform))
        .route("/ui/platforms", get(list_platforms))
        .route("/ui/platforms/{id}", get(get_platform))
        .route("/ui/platforms/{id}", put(update_platform))
        .route("/ui/platforms/{id}", delete(delete_platform))
        .route("/ui/platforms/{id}/devices", get(list_platform_devices))
        .route(
            "/ui/platforms/{id}/devices/details",
            get(get_platform_devices_with_details),
        )
        .with_state(state)
}

/// Validate platform creation request
fn validate_create_platform(req: &CreatePlatformRequest) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    errors.add_if_err("name", validate_required(&req.name, "Name"));
    errors.add_if_err("name", validate_string_length(&req.name, 255, "Name"));

    if let Some(desc) = &req.description {
        errors.add_if_err(
            "description",
            validate_string_length(desc, 1000, "Description"),
        );
    }

    // Validate platform attributes
    validate_platform_attributes(&req.attributes, &mut errors);

    errors.into_result()
}

/// Validate platform update request
fn validate_update_platform(req: &UpdatePlatformRequest) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    if let Some(name) = &req.name {
        errors.add_if_err("name", validate_required(name, "Name"));
        errors.add_if_err("name", validate_string_length(name, 255, "Name"));
    }

    if let Some(desc) = &req.description {
        errors.add_if_err(
            "description",
            validate_string_length(desc, 1000, "Description"),
        );
    }

    if let Some(attrs) = &req.attributes {
        validate_platform_attributes(attrs, &mut errors);
    }

    errors.into_result()
}

/// Validate platform attributes structure
fn validate_platform_attributes(attrs: &PlatformAttributes, errors: &mut ValidationErrors) {
    // Validate at least one disk
    if attrs.disks.is_empty() {
        errors.add_error("disks", "At least one disk is required".to_string());
    }

    // Validate disk types
    for (i, disk) in attrs.disks.iter().enumerate() {
        if disk.path.is_empty() {
            errors.add_error(
                &format!("disks[{}].path", i),
                "Disk path is required".to_string(),
            );
        }
        if disk.size_gb == 0 {
            errors.add_error(
                &format!("disks[{}].size_gb", i),
                "Disk size must be greater than 0".to_string(),
            );
        }
    }

    // Validate disk labels are unique
    let mut seen_labels = std::collections::HashSet::new();
    for (i, disk) in attrs.disks.iter().enumerate() {
        if let Some(label) = &disk.label
            && !seen_labels.insert(label.clone())
        {
            errors.add_error(
                &format!("disks[{}].label", i),
                format!("Duplicate disk label: {}", label),
            );
        }
    }

    // Validate at least one NIC
    if attrs.nics.is_empty() {
        errors.add_error("nics", "At least one NIC is required".to_string());
    }

    // Validate NIC labels are unique
    let mut seen_nic_labels = std::collections::HashSet::new();
    for (i, nic) in attrs.nics.iter().enumerate() {
        if nic.logical.is_empty() {
            errors.add_error(
                &format!("nics[{}].logical", i),
                "NIC logical name is required".to_string(),
            );
        }
        if let Some(label) = &nic.label
            && !seen_nic_labels.insert(label.clone())
        {
            errors.add_error(
                &format!("nics[{}].label", i),
                format!("Duplicate NIC label: {}", label),
            );
        }
    }

    // Validate at least one CPU
    if attrs.cpus.is_empty() {
        errors.add_error("cpus", "At least one CPU is required".to_string());
    }

    for (i, cpu) in attrs.cpus.iter().enumerate() {
        if cpu.brand.is_empty() {
            errors.add_error(
                &format!("cpus[{}].brand", i),
                "CPU brand is required".to_string(),
            );
        }
        if cpu.model.is_empty() {
            errors.add_error(
                &format!("cpus[{}].model", i),
                "CPU model is required".to_string(),
            );
        }
        if cpu.cores == 0 {
            errors.add_error(
                &format!("cpus[{}].cores", i),
                "CPU cores must be greater than 0".to_string(),
            );
        }
    }

    // Validate memory
    if attrs.memory_gib == 0 {
        errors.add_error("memory_gib", "Memory must be greater than 0".to_string());
    }
}

// Create a new platform
async fn create_platform(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePlatformRequest>,
) -> Result<(StatusCode, Json<Platform>), HttpError> {
    if let Err(errors) = validate_create_platform(&req) {
        return Err(HttpError::ValidationError(errors));
    }

    let platform = state
        .platforms_store
        .create(&req.name, req.description.as_deref(), &req.attributes)
        .await?;

    Ok((StatusCode::CREATED, Json(platform)))
}

// List all platforms
async fn list_platforms(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Platform>>, HttpError> {
    let platforms = state.platforms_store.list().await?;
    Ok(Json(platforms))
}

// Get a specific platform
async fn get_platform(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Platform>, HttpError> {
    let platform = state.platforms_store.get(id).await?;
    Ok(Json(platform))
}

// Update a platform
async fn update_platform(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdatePlatformRequest>,
) -> Result<Json<Platform>, HttpError> {
    if let Err(errors) = validate_update_platform(&req) {
        return Err(HttpError::ValidationError(errors));
    }

    let platform = state
        .platforms_store
        .update(
            id,
            req.name.as_deref(),
            req.description.as_deref(),
            req.attributes.as_ref(),
        )
        .await?;

    Ok(Json(platform))
}

// Delete a platform
async fn delete_platform(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.platforms_store.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// List all devices with a specific platform
async fn list_platform_devices(
    State(state): State<Arc<AppState>>,
    Path(platform_id): Path<i64>,
) -> Result<Json<Vec<String>>, HttpError> {
    let devices = state
        .director
        .list_devices_with_platform(platform_id)
        .await?;
    let device_strs: Vec<String> = devices.iter().map(|u| u.to_string()).collect();
    Ok(Json(device_strs))
}

#[derive(Serialize)]
struct PlatformDeviceInfo {
    uuid: String,
    hostname: Option<String>,
    lifecycle: Option<String>,
}

/// Get detailed device information for all devices assigned to a platform
///
/// Returns device UUID, hostname, and lifecycle state for each device
/// assigned to the specified platform.
async fn get_platform_devices_with_details(
    State(state): State<Arc<AppState>>,
    Path(platform_id): Path<i64>,
) -> Result<Json<Vec<PlatformDeviceInfo>>, HttpError> {
    let device_uuids = state
        .director
        .list_devices_with_platform(platform_id)
        .await?;

    let mut devices_info = Vec::new();
    for uuid in device_uuids {
        // Get device info
        let device = state.director.get_device(&uuid).await?;
        let lifecycle = state.director.get_device_lifecycle(&uuid).await?;

        devices_info.push(PlatformDeviceInfo {
            uuid: uuid.to_string(),
            hostname: device.attributes.hostname,
            lifecycle: lifecycle.map(String::from),
        });
    }

    Ok(Json(devices_info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database,
        director::Director,
        operating_systems::Architecture,
        platforms::{DiskType, PlatformCpu, PlatformDisk, PlatformNic},
    };
    use rusqlite::Connection;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    fn setup_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        database::run_migrations(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn test_uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-0000000000{:02x}", n))
            .expect("valid UUID")
    }

    fn sample_platform_attributes() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    path: "/dev/disk/by-path/pci-0000:00:1f.2-ata-2".to_string(),
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![
                PlatformNic {
                    logical: "eno1".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC1".to_string()),
                },
                PlatformNic {
                    logical: "eno2".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC2".to_string()),
                },
            ],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        }
    }

    #[tokio::test]
    async fn test_get_platform_devices_with_details_empty() {
        let db = setup_db();
        let platforms_store = PlatformsStore::new(db.clone());
        let director = Director::new(db);

        // Create a platform
        let attrs = sample_platform_attributes();
        let platform = platforms_store
            .create("Test Platform", Some("Test Description"), &attrs)
            .await
            .unwrap();

        let platform_id = platform.id.unwrap();

        // Get devices for platform (should be empty)
        let devices = director
            .list_devices_with_platform(platform_id)
            .await
            .unwrap();

        assert_eq!(devices.len(), 0);
    }

    #[tokio::test]
    async fn test_get_platform_devices_with_details_single_device() {
        let db = setup_db();
        let platforms_store = PlatformsStore::new(db.clone());
        let director = Director::new(db);

        // Create a platform
        let attrs = sample_platform_attributes();
        let platform = platforms_store
            .create("Test Platform", Some("Test Description"), &attrs)
            .await
            .unwrap();

        let platform_id = platform.id.unwrap();

        // Register a device
        let device_uuid = test_uuid(1);
        director
            .register_device(&device_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Assign platform to device
        director
            .assign_platform_to_device(&device_uuid, platform_id)
            .await
            .unwrap();

        // Get devices for platform
        let devices = director
            .list_devices_with_platform(platform_id)
            .await
            .unwrap();

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0], device_uuid);

        // Get device details
        let device = director.get_device(&device_uuid).await.unwrap();
        let lifecycle = director.get_device_lifecycle(&device_uuid).await.unwrap();

        // Verify the data we would return from the endpoint
        assert_eq!(device.uuid, device_uuid);
        // Hostname might be auto-assigned during registration
        assert!(device.attributes.hostname.is_some());
        assert!(lifecycle.is_some()); // Should have a lifecycle state after registration
    }

    #[tokio::test]
    async fn test_get_platform_devices_with_details_multiple_devices() {
        let db = setup_db();
        let platforms_store = PlatformsStore::new(db.clone());
        let director = Director::new(db);

        // Create a platform
        let attrs = sample_platform_attributes();
        let platform = platforms_store
            .create("Test Platform", Some("Test Description"), &attrs)
            .await
            .unwrap();

        let platform_id = platform.id.unwrap();

        // Register multiple devices
        let device1_uuid = test_uuid(1);
        let device2_uuid = test_uuid(2);
        let device3_uuid = test_uuid(3);

        director
            .register_device(&device1_uuid, Architecture::X86_64)
            .await
            .unwrap();
        director
            .register_device(&device2_uuid, Architecture::X86_64)
            .await
            .unwrap();
        director
            .register_device(&device3_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Assign platform to all devices
        director
            .assign_platform_to_device(&device1_uuid, platform_id)
            .await
            .unwrap();
        director
            .assign_platform_to_device(&device2_uuid, platform_id)
            .await
            .unwrap();
        director
            .assign_platform_to_device(&device3_uuid, platform_id)
            .await
            .unwrap();

        // Set hostname on one device by updating its attributes
        let mut updated_attrs = serde_json::Map::new();
        updated_attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("test-hostname".to_string()),
        );
        director
            .update_attributes(&device2_uuid, updated_attrs)
            .await
            .unwrap();

        // Get devices for platform
        let devices = director
            .list_devices_with_platform(platform_id)
            .await
            .unwrap();

        assert_eq!(devices.len(), 3);

        // Verify we can get details for all devices
        for uuid in &devices {
            let device = director.get_device(uuid).await.unwrap();
            let lifecycle = director.get_device_lifecycle(uuid).await.unwrap();

            assert!(lifecycle.is_some());

            // Check that device2 has the hostname we set
            if *uuid == device2_uuid {
                assert_eq!(
                    device.attributes.hostname,
                    Some("test-hostname".to_string())
                );
            }
        }
    }

    #[tokio::test]
    async fn test_get_platform_devices_with_details_lifecycle_states() {
        let db = setup_db();
        let platforms_store = PlatformsStore::new(db.clone());
        let director = Director::new(db);

        // Create a platform
        let attrs = sample_platform_attributes();
        let platform = platforms_store
            .create("Test Platform", Some("Test Description"), &attrs)
            .await
            .unwrap();

        let platform_id = platform.id.unwrap();

        // Register a device
        let device_uuid = test_uuid(1);
        director
            .register_device(&device_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Assign platform to device
        director
            .assign_platform_to_device(&device_uuid, platform_id)
            .await
            .unwrap();

        // Get lifecycle and verify it can be converted to string
        let lifecycle = director.get_device_lifecycle(&device_uuid).await.unwrap();
        assert!(lifecycle.is_some());

        let lifecycle_str = lifecycle.map(String::from);
        assert!(lifecycle_str.is_some());

        // Verify the string representation is one of the valid lifecycle states
        let lifecycle_value = lifecycle_str.unwrap();
        assert!(
            lifecycle_value == "new"
                || lifecycle_value == "unprovisioned"
                || lifecycle_value == "provisioned"
                || lifecycle_value == "removed"
                || lifecycle_value == "broken"
        );
    }

    #[tokio::test]
    async fn test_get_platform_devices_with_details_nonexistent_platform() {
        let db = setup_db();
        let director = Director::new(db);

        // Try to get devices for a nonexistent platform
        let result = director.list_devices_with_platform(999).await;

        // Should return an empty list (not an error)
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
