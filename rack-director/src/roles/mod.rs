mod store;

pub use store::RolesStore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::FromRow;

/// A Role defines how a device should be provisioned
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub os_id: i64,
    pub disk_layout: DiskLayout,
    pub config_template: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl FromRow for Role {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let disk_layout_json: String = row.get("disk_layout")?;
        let disk_layout: DiskLayout = serde_json::from_str(&disk_layout_json).unwrap();
        let config_json: Option<String> = row.get("config_template")?;
        let config_template = config_json.and_then(|s| serde_json::from_str(&s).ok());

        Ok(Role {
            id: row.get("id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            os_id: row.get("os_id")?,
            disk_layout,
            config_template,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// Disk layout configuration defining partition scheme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskLayout {
    pub partitions: Vec<Partition>,
}

/// A disk partition configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partition {
    pub device: String,
    pub size: String,
    pub filesystem: String,
    pub mount_point: Option<String>,
    pub flags: Vec<String>,
}

/// Request to create a new role
#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub os_id: i64,
    pub disk_layout: DiskLayout,
    pub config_template: Option<serde_json::Value>,
}

/// Request to update a role
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub os_id: Option<i64>,
    pub disk_layout: Option<DiskLayout>,
    pub config_template: Option<serde_json::Value>,
}

/// Request to assign a role to a device
#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: i64,
}

/// Role with associated operating system information
#[derive(Debug, Serialize)]
pub struct RoleWithOs {
    #[serde(flatten)]
    pub role: Role,
    pub os_name: String,
    pub os_version: String,
}
