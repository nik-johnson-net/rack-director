mod boot_target;
pub mod params;

pub use boot_target::BootTarget;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::database::Connection;
use crate::director::Device;

// TODO: Define common AgentAction enum.

/// Strongly-typed Action enum representing all possible provisioning actions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    DiscoverHardware,
    ConfigureBmc,
    InstallOs,
    PartitionDisks,
    RebootDevice,
}

/// Context required for converting Actions to BootTargets
pub struct ActionContext<'a> {
    pub device: &'a Device,
    pub conn: &'a Connection,
    pub director: Option<&'a crate::director::Director<'a>>,
}

impl Action {
    /// Convert this Action to a BootTarget for the device
    ///
    /// This method determines what the device should boot into based on the current action.
    /// For most actions, this returns LocalDisk. For special actions like hardware discovery,
    /// BMC configuration, and OS installation, it returns NetBoot with appropriate kernel/initramfs.
    pub async fn to_boot_target(&self, ctx: &ActionContext<'_>) -> Result<BootTarget> {
        match self {
            Action::DiscoverHardware | Action::ConfigureBmc | Action::PartitionDisks => {
                generate_agent_boot_target("daemon")
            }
            Action::InstallOs => generate_os_install_boot_target(ctx).await,
            // All other actions default to local disk boot
            _ => Ok(BootTarget::LocalDisk),
        }
    }

    /// Execute startup logic when transitioning to this action
    ///
    /// Some actions need to trigger side effects when they become active.
    /// For example, RebootDevice needs to send an IPMI power reset command.
    /// Most actions don't need startup logic and return Ok(()).
    pub async fn start(&self, ctx: &ActionContext<'_>) -> Result<()> {
        match self {
            Action::RebootDevice => {
                // Trigger reboot via IPMI
                if let Some(director) = ctx.director {
                    log::debug!("RebootDevice action started for device {}", ctx.device.uuid);
                    director.reboot(&ctx.device.uuid).await?;
                } else {
                    log::warn!(
                        "Cannot execute RebootDevice action: director not available in context"
                    );
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Check if this action should auto-advance when the device boots
    ///
    /// Some actions (like RebootDevice) are complete once the device boots.
    /// Other actions require explicit success/failure reporting from the agent.
    ///
    /// Returns true if the action should automatically advance to the next
    /// step when director.on_boot() is called.
    pub fn advance_on_boot(&self) -> bool {
        #[allow(clippy::match_like_matches_macro)]
        match self {
            Action::RebootDevice => true,
            _ => false,
        }
    }
}

/// Console and debugging kernel arguments shared by both the agent image boot and the OS
/// installer boot. These args ensure serial console output is available for debugging
/// regardless of which boot stage is running.
const DEFAULT_LINUX_CMDLINE: &str =
    "console=ttyS0 console=ttyS1,115200n8 earlyprintk=serial,ttyS0,115200n8";

/// Prepend the default Linux cmdline args to an OS-provided cmdline string.
///
/// When the OS provides its own cmdline args, the defaults are prepended so that
/// OS-specific args can override or extend the defaults. When the OS provides no
/// cmdline args, only the defaults are used.
fn prepend_default_cmdline(os_cmdline: Option<String>) -> String {
    match os_cmdline {
        Some(args) if !args.is_empty() => format!("{DEFAULT_LINUX_CMDLINE} {args}"),
        _ => DEFAULT_LINUX_CMDLINE.to_string(),
    }
}

/// Generate boot target for agent-based actions (discovery, BMC config, etc.)
fn generate_agent_boot_target(action_name: &str) -> Result<BootTarget> {
    let cmdline = format!("ro no_timer_check {DEFAULT_LINUX_CMDLINE}");

    Ok(BootTarget::AgentImage {
        action: action_name.into(),
        cmdline,
    })
}

/// Generate boot target for OS installation
async fn generate_os_install_boot_target(ctx: &ActionContext<'_>) -> Result<BootTarget> {
    // Get device architecture from the passed device
    let architecture = ctx.device.architecture;

    // Get device role
    let role_id = ctx
        .device
        .role_id
        .ok_or_else(|| anyhow::anyhow!("role not assigned to device {}", ctx.device.uuid))?;

    let role = crate::roles::store::get(ctx.conn, role_id).await?;

    // Get OS architecture configuration
    let os_arch =
        crate::operating_systems::store::get_architecture(ctx.conn, role.os_id, architecture)
            .await?;

    let cmdline = prepend_default_cmdline(os_arch.cmdline_args);

    Ok(BootTarget::NetBoot {
        ramdisk: os_arch.initramfs_path.clone(),
        kernel: os_arch.kernel_path.clone(),
        cmdline,
        modules: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::Device;
    use crate::operating_systems::Architecture;
    use crate::roles::DiskLayout;
    use crate::{
        database, database::DatabaseConnectionFactory, operating_systems::store as os_store,
        roles::store as roles_store, test_connection_factory,
    };
    use std::sync::Arc;
    use uuid::Uuid;

    /// Helper to create test database for ActionContext
    async fn setup_test_db(factory: DatabaseConnectionFactory) -> Arc<crate::database::Connection> {
        Arc::new(database::run_migrations(&factory).await.unwrap())
    }

    #[test]
    fn test_action_serialization() {
        // Test that Action serializes with tagged format
        let action = Action::DiscoverHardware;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"discover_hardware"}"#);

        let action = Action::InstallOs;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"install_os"}"#);

        let action = Action::ConfigureBmc;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"configure_bmc"}"#);
    }

    #[test]
    fn test_action_deserialization() {
        // Test that Action deserializes from tagged format
        let json = r#"{"type":"discover_hardware"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::DiscoverHardware);

        let json = r#"{"type":"install_os"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::InstallOs);

        let json = r#"{"type":"partition_disks"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::PartitionDisks);
    }

    #[test]
    fn test_all_action_variants_serialize() {
        // Ensure all variants can be serialized without panic
        let actions = vec![
            Action::DiscoverHardware,
            Action::ConfigureBmc,
            Action::InstallOs,
            Action::PartitionDisks,
            Action::RebootDevice,
        ];

        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            assert!(json.contains(r#"{"type":"#));
        }
    }

    #[test]
    fn test_generate_agent_boot_target() {
        let boot_target = generate_agent_boot_target("daemon").unwrap();

        match boot_target {
            BootTarget::AgentImage { action, cmdline } => {
                assert_eq!(action, "daemon");
                // Agent-specific args precede the shared console/debugging defaults.
                assert_eq!(
                    cmdline,
                    format!("ro no_timer_check {DEFAULT_LINUX_CMDLINE}")
                );
            }
            _ => panic!("Expected AgentImage, got something else"),
        }
    }

    #[tokio::test]
    async fn test_install_os_action_to_boot_target_success() {
        let conn = setup_test_db(test_connection_factory!()).await;

        // Create an OS with architecture
        let os = os_store::create(&conn, "Ubuntu", "24.04", Some("Ubuntu 24.04 LTS"))
            .await
            .unwrap();
        let os_id = os.id.unwrap();

        // Add architecture configuration with cmdline template
        os_store::upsert_architecture(
            &conn,
            os_id,
            Architecture::X86_64,
            "installer/ubuntu-24.04/vmlinuz",
            "installer/ubuntu-24.04/initrd.img",
            vec![],
            Some("console=ttyS0 autoinstall ds=nocloud-net;s={{install_script_url}}"),
            None,
        )
        .await
        .unwrap();

        // Create a role
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = roles_store::create(
            &conn,
            "web-server",
            Some("Web server role"),
            os_id,
            &disk_layout,
            None,
        )
        .await
        .unwrap();
        let role_id = role.id.unwrap();

        // Create and register device with role
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        conn.execute(
            "INSERT INTO devices (uuid, architecture, lifecycle, role_id) VALUES (?1, 'x86-64', 'new', ?2)",
            (device_uuid, role_id),
        )
        .await
        .unwrap();

        let device = Device {
            id: 0,
            uuid: device_uuid,
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: Some(role_id),
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::InstallOs;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
                modules: _,
            } => {
                assert!(kernel.contains("installer/ubuntu-24.04/vmlinuz"));
                assert!(ramdisk.contains("installer/ubuntu-24.04/initrd.img"));
                assert!(cmdline.contains("console=ttyS0"));
                assert!(cmdline.contains("autoinstall"));
                assert!(cmdline.contains("ds=nocloud-net;s={{install_script_url}}"));
            }
            _ => panic!("Expected NetBoot for InstallOs, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_install_os_action_missing_role_error() {
        let conn = setup_test_db(test_connection_factory!()).await;

        // Create device without a role
        let device = Device {
            id: 0,
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::InstallOs;
        let result = action.to_boot_target(&ctx).await;

        assert!(result.is_err(), "Expected error when device has no role");
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("role not assigned"),
            "Error message should mention missing role, got: {}",
            error_msg
        );
    }

    #[tokio::test]
    async fn test_install_os_action_with_no_cmdline_template() {
        let conn = setup_test_db(test_connection_factory!()).await;

        // Create an OS with architecture but NO cmdline template
        let os = os_store::create(&conn, "Debian", "12", Some("Debian 12"))
            .await
            .unwrap();
        let os_id = os.id.unwrap();

        // Add architecture configuration WITHOUT cmdline template
        os_store::upsert_architecture(
            &conn,
            os_id,
            Architecture::X86_64,
            "installer/debian-12/vmlinuz",
            "installer/debian-12/initrd.img",
            vec![],
            None, // No cmdline template
            None,
        )
        .await
        .unwrap();

        // Create a role
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = roles_store::create(
            &conn,
            "database-server",
            Some("Database server role"),
            os_id,
            &disk_layout,
            None,
        )
        .await
        .unwrap();
        let role_id = role.id.unwrap();

        // Create and register device with role
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440005").unwrap();
        conn.execute(
            "INSERT INTO devices (uuid, architecture, lifecycle, role_id) VALUES (?1, 'x86-64', 'new', ?2)",
            (device_uuid, role_id),
        )
        .await
        .unwrap();

        let device = Device {
            id: 0,
            uuid: device_uuid,
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: Some(role_id),
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::InstallOs;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
                modules: _,
            } => {
                assert!(kernel.contains("installer/debian-12/vmlinuz"));
                assert!(ramdisk.contains("installer/debian-12/initrd.img"));
                // When no OS cmdline is provided, the cmdline should equal only the defaults.
                assert_eq!(
                    cmdline, DEFAULT_LINUX_CMDLINE,
                    "Expected default cmdline when no OS cmdline provided"
                );
            }
            _ => panic!("Expected NetBoot for InstallOs, got LocalDisk"),
        }
    }

    #[test]
    fn test_reboot_device_serialization() {
        let action = Action::RebootDevice;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"reboot_device"}"#);
    }

    #[test]
    fn test_reboot_device_deserialization() {
        let json = r#"{"type":"reboot_device"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::RebootDevice);
    }

    #[test]
    fn test_advance_on_boot_reboot_device() {
        // RebootDevice should advance on boot
        assert!(Action::RebootDevice.advance_on_boot());
    }

    #[test]
    fn test_advance_on_boot_other_actions() {
        // Other actions should NOT advance on boot
        assert!(!Action::DiscoverHardware.advance_on_boot());
        assert!(!Action::ConfigureBmc.advance_on_boot());
        assert!(!Action::InstallOs.advance_on_boot());
        assert!(!Action::PartitionDisks.advance_on_boot());
    }

    #[tokio::test]
    async fn test_reboot_device_start() {
        let conn = setup_test_db(test_connection_factory!()).await;

        let device = Device {
            id: 0,
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440007").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::RebootDevice;
        // start() should succeed without error
        let result = action.start(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reboot_device_boot_target() {
        let conn = setup_test_db(test_connection_factory!()).await;

        let device = Device {
            id: 0,
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440008").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::RebootDevice;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        // RebootDevice should boot to local disk (the action is just to trigger the reboot)
        match boot_target {
            BootTarget::LocalDisk => {} // Expected
            _ => {
                panic!("Expected LocalDisk for RebootDevice, got NetBoot")
            }
        }
    }

    #[tokio::test]
    async fn test_partition_disks_boot_target() {
        let conn = setup_test_db(test_connection_factory!()).await;

        let device = Device {
            id: 0,
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440009").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::PartitionDisks;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::AgentImage { action, cmdline } => {
                assert_eq!(action, "daemon");
                assert!(cmdline.contains("console=ttyS1"));
            }
            _ => panic!(
                "Expected AgentImage for PartitionDisks, got {:?}",
                boot_target
            ),
        }
    }
}
