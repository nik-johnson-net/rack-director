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
    WipeDisks,
    RebootDevice,
    Console,
    /// Boot the freshly installed OS from local disk and wait for it to report that its
    /// first boot (cloud-init) finished. Only planned for OSes whose OSM declares `cloud_init`.
    FirstBoot,
}

/// Context required for converting Actions to BootTargets
pub struct ActionContext<'a> {
    pub device: &'a Device,
    pub conn: &'a Connection,
    pub director: Option<&'a crate::director::Director<'a>>,
}

impl Action {
    /// The BootTarget a device should boot while this action is current.
    ///
    /// The target follows whatever runs the action: the rack-agent image for agent-driven
    /// actions, the OS installer for `InstallOs`, and the local disk when the installed OS
    /// (or a plain reboot) takes over.
    pub async fn to_boot_target(&self, ctx: &ActionContext<'_>) -> Result<BootTarget> {
        match self {
            Action::DiscoverHardware
            | Action::ConfigureBmc
            | Action::PartitionDisks
            | Action::WipeDisks => generate_agent_boot_target("daemon"),
            Action::InstallOs => generate_os_install_boot_target(ctx).await,
            Action::Console => generate_agent_boot_target("console"),
            // FirstBoot boots the installed OS, never the agent or the installer again.
            Action::RebootDevice | Action::FirstBoot => Ok(BootTarget::LocalDisk),
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

    /// Parse an action from its wire name (`install_os`, `first_boot`, ...): the `type` tag it
    /// serializes with. Reports from install templates and the first-boot reporter name the
    /// step they finish this way. `None` for an unknown name.
    pub fn from_wire_name(name: &str) -> Option<Action> {
        serde_json::from_value(serde_json::json!({ "type": name })).ok()
    }
}

/// Console and debugging kernel arguments shared by both the agent image boot and the OS
/// installer boot. These args ensure serial console output is available for debugging
/// regardless of which boot stage is running.
const DEFAULT_LINUX_CMDLINE: &str = "console=ttyS1,115200n8";

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

/// Boot target for a registered device with no work to do: the agent idles in daemon mode,
/// polling for a plan, so a new plan starts in-band without a reboot or power kick.
pub fn idle_daemon_boot_target() -> Result<BootTarget> {
    generate_agent_boot_target("daemon")
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
    // Get device role
    let role_id = ctx
        .device
        .role_id
        .ok_or_else(|| anyhow::anyhow!("role not assigned to device {}", ctx.device.uuid))?;

    let role = crate::roles::store::get(ctx.conn, role_id).await?;

    // Resolve OS from OSM registry
    let resolved = role.resolve_os(ctx.conn).await?;

    // Build cmdline: prepend defaults, then OS cmdline, then role cmdline
    let os_cmdline = resolved.cmdline().to_string();
    let mut cmdline = prepend_default_cmdline(if os_cmdline.is_empty() {
        None
    } else {
        Some(os_cmdline)
    });

    // Append role-level cmdline args if present
    if let Some(ref role_cmdline) = role.cmdline_args
        && !role_cmdline.is_empty()
    {
        cmdline = format!("{} {}", cmdline, role_cmdline);
    }

    Ok(BootTarget::NetBoot {
        ramdisk: resolved.initramfs_storage_path(),
        kernel: resolved.kernel_storage_path(),
        cmdline,
        modules: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::Architecture;
    use crate::director::Device;
    use crate::director::test_support::{create_default_module, os_config};
    use crate::roles::DiskLayout;
    use crate::{
        database, database::DatabaseConnectionFactory, roles::store as roles_store,
        test_connection_factory,
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
    fn test_wipe_disks_serialization() {
        let action = Action::WipeDisks;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"wipe_disks"}"#);
    }

    #[test]
    fn test_wipe_disks_deserialization() {
        let json = r#"{"type":"wipe_disks"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::WipeDisks);
    }

    /// Compile-time guard ensuring [`test_all_action_variants_serialize`] covers
    /// every `Action` variant. Adding a new variant to `Action` without adding
    /// it here (and to the `actions` array below) fails to compile, since this
    /// match has no wildcard arm.
    fn assert_variant_covered(action: &Action) {
        match action {
            Action::DiscoverHardware
            | Action::ConfigureBmc
            | Action::InstallOs
            | Action::PartitionDisks
            | Action::WipeDisks
            | Action::RebootDevice
            | Action::Console
            | Action::FirstBoot => {}
        }
    }

    #[test]
    fn test_all_action_variants_serialize() {
        // Ensure all variants can be serialized without panic
        let actions = vec![
            Action::DiscoverHardware,
            Action::ConfigureBmc,
            Action::InstallOs,
            Action::PartitionDisks,
            Action::WipeDisks,
            Action::RebootDevice,
            Action::Console,
            Action::FirstBoot,
        ];

        for action in &actions {
            assert_variant_covered(action);
            let json = serde_json::to_string(action).unwrap();
            assert!(json.contains(r#"{"type":"#));
        }
    }

    #[test]
    fn test_console_serialization() {
        let action = Action::Console;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"console"}"#);
    }

    #[test]
    fn test_first_boot_serialization_roundtrip() {
        let json = serde_json::to_string(&Action::FirstBoot).unwrap();
        assert_eq!(json, r#"{"type":"first_boot"}"#);
        let action: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(action, Action::FirstBoot);
        assert!(!action.advance_on_boot());
    }

    #[test]
    fn test_from_wire_name() {
        assert_eq!(
            Action::from_wire_name("install_os"),
            Some(Action::InstallOs)
        );
        assert_eq!(
            Action::from_wire_name("first_boot"),
            Some(Action::FirstBoot)
        );
        assert_eq!(Action::from_wire_name("InstallOs"), None);
        assert_eq!(Action::from_wire_name("reboot"), None);
    }

    #[test]
    fn test_console_deserialization() {
        let json = r#"{"type":"console"}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(action, Action::Console);
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

        // Set up OSM module with an OS entry
        create_default_module(&conn).await;
        let mut config = os_config("TestOS", "1.0", false);
        config.architectures[0].cmdline =
            "console=ttyS0 autoinstall ds=nocloud-net;s={{install_script_url}}".to_string();
        crate::osm::store::create_operating_system(&conn, 1, "testos", "TestOS", "1.0", &config)
            .await
            .unwrap();

        // Create a role referencing the OSM module
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let role = roles_store::create(
            &conn,
            "web-server",
            Some("Web server role"),
            "Default",
            "TestOS",
            "1.0",
            "x86-64",
            &disk_layout,
            None,
            None,
            None,
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
            last_polled_at: None,
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
                assert_eq!(kernel, "osm/Default/1.0.0/testos/vmlinuz");
                assert_eq!(ramdisk, "osm/Default/1.0.0/testos/initrd.img");
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
            last_polled_at: None,
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

        // Set up OSM module with an OS entry that has NO cmdline args
        create_default_module(&conn).await;
        let config = os_config("Debian", "12", false); // No cmdline template
        crate::osm::store::create_operating_system(&conn, 1, "debian", "Debian", "12", &config)
            .await
            .unwrap();

        // Create a role referencing the OSM module
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        };
        let role = roles_store::create(
            &conn,
            "database-server",
            Some("Database server role"),
            "Default",
            "Debian",
            "12",
            "x86-64",
            &disk_layout,
            None,
            None,
            None,
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
            last_polled_at: None,
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
                assert_eq!(kernel, "osm/Default/1.0.0/debian/vmlinuz");
                assert_eq!(ramdisk, "osm/Default/1.0.0/debian/initrd.img");
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
        assert!(!Action::WipeDisks.advance_on_boot());
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
            last_polled_at: None,
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
            last_polled_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        // RebootDevice only triggers the reboot; FirstBoot boots the installed OS.
        for action in [Action::RebootDevice, Action::FirstBoot] {
            let boot_target = action.to_boot_target(&ctx).await.unwrap();
            assert!(
                matches!(boot_target, BootTarget::LocalDisk),
                "Expected LocalDisk for {action:?}, got {boot_target:?}"
            );
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
            last_polled_at: None,
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

    #[tokio::test]
    async fn test_wipe_disks_boot_target() {
        let conn = setup_test_db(test_connection_factory!()).await;

        let device = Device {
            id: 0,
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440021").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            platform_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
            last_polled_at: None,
        };

        let ctx = ActionContext {
            device: &device,
            conn: &conn,
            director: None,
        };

        let action = Action::WipeDisks;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::AgentImage { action, cmdline } => {
                assert_eq!(action, "daemon");
                assert!(cmdline.contains("console=ttyS1"));
            }
            _ => panic!("Expected AgentImage for WipeDisks, got {:?}", boot_target),
        }
    }
}
