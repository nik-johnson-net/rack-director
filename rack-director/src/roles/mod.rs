pub mod store;

pub use common::disk_layout::DiskLayout;

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
    /// Required firmware mode for devices assigned to this role.
    /// If set, only devices with the matching boot_mode can be assigned this role.
    /// None means no firmware constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_mode: Option<common::FirmwareMode>,
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

        let firmware_mode_str: Option<String> = row.get("firmware_mode")?;
        let firmware_mode = firmware_mode_str.and_then(|s| match s.as_str() {
            "bios" => Some(common::FirmwareMode::Bios),
            "uefi" => Some(common::FirmwareMode::Uefi),
            _ => None,
        });

        Ok(Role {
            id: row.get("id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            os_id: row.get("os_id")?,
            disk_layout,
            config_template,
            firmware_mode,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// Request to create a new role
#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub os_id: i64,
    pub disk_layout: DiskLayout,
    pub config_template: Option<serde_json::Value>,
    pub firmware_mode: Option<common::FirmwareMode>,
}

/// Request to update a role
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub os_id: Option<i64>,
    pub disk_layout: Option<DiskLayout>,
    pub config_template: Option<serde_json::Value>,
    pub firmware_mode: Option<common::FirmwareMode>,
    /// When true, clears firmware_mode to NULL regardless of the firmware_mode field.
    /// Use this to remove a firmware constraint from a role.
    #[serde(default)]
    pub clear_firmware_mode: bool,
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
