pub mod store;

pub use common::disk_layout::DiskLayout;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::FromRow;
use crate::patch::Patch;

/// A Role defines how a device should be provisioned
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    /// OSM module name (e.g., "Default")
    pub osm_module: String,
    /// OS name within the module (e.g., "Ubuntu")
    pub os_name: String,
    /// OS release (e.g., "22.04")
    pub os_release: String,
    /// Architecture (e.g., "x86-64")
    pub os_arch: String,
    pub disk_layout: DiskLayout,
    pub cmdline_args: Option<String>,
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
            osm_module: row.get("osm_module")?,
            os_name: row.get("os_name")?,
            os_release: row.get("os_release")?,
            os_arch: row.get("os_arch")?,
            disk_layout,
            cmdline_args: row.get("cmdline_args")?,
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
    /// OSM module name (e.g., "Default")
    pub osm_module: String,
    /// OS name within the module (e.g., "Ubuntu")
    pub os_name: String,
    /// OS release (e.g., "22.04")
    pub os_release: String,
    /// Architecture (e.g., "x86-64")
    pub os_arch: String,
    pub disk_layout: DiskLayout,
    pub cmdline_args: Option<String>,
    pub config_template: Option<serde_json::Value>,
    pub firmware_mode: Option<common::FirmwareMode>,
}

/// Request to update a role.
///
/// Fields backed by nullable columns use [`Patch`], which distinguishes an absent key (keep)
/// from an explicit `null` (clear). The struct-level `#[serde(default)]` is mandatory — see
/// the `crate::patch` module docs.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Patch<String>,
    /// OSM module name (e.g., "Default")
    pub osm_module: Option<String>,
    /// OS name within the module (e.g., "Ubuntu")
    pub os_name: Option<String>,
    /// OS release (e.g., "22.04")
    pub os_release: Option<String>,
    /// Architecture (e.g., "x86-64")
    pub os_arch: Option<String>,
    pub disk_layout: Option<DiskLayout>,
    pub cmdline_args: Patch<String>,
    pub config_template: Patch<serde_json::Value>,
    pub firmware_mode: Patch<common::FirmwareMode>,
}

/// Request to assign a role to a device
#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_update(json: &str) -> UpdateRoleRequest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_update_role_request_absent_patch_fields_are_keep() {
        let req = parse_update(r#"{"name": "web"}"#);
        assert_eq!(req.description, Patch::Keep);
        assert_eq!(req.cmdline_args, Patch::Keep);
        assert_eq!(req.config_template, Patch::Keep);
        assert_eq!(req.firmware_mode, Patch::Keep);
    }

    #[test]
    fn test_update_role_request_null_patch_fields_are_clear() {
        let req = parse_update(
            r#"{"description": null, "cmdline_args": null, "config_template": null, "firmware_mode": null}"#,
        );
        assert_eq!(req.description, Patch::Clear);
        assert_eq!(req.cmdline_args, Patch::Clear);
        assert_eq!(req.config_template, Patch::Clear);
        assert_eq!(req.firmware_mode, Patch::Clear);
    }

    #[test]
    fn test_update_role_request_valued_patch_fields_are_set() {
        let req = parse_update(
            r#"{"description": "d", "cmdline_args": "quiet", "config_template": {"k": 1}, "firmware_mode": "uefi"}"#,
        );
        assert_eq!(req.description, Patch::Set("d".to_string()));
        assert_eq!(req.cmdline_args, Patch::Set("quiet".to_string()));
        assert_eq!(req.config_template, Patch::Set(serde_json::json!({"k": 1})));
        assert_eq!(req.firmware_mode, Patch::Set(common::FirmwareMode::Uefi));
    }
}
