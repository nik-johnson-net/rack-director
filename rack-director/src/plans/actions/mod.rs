pub mod params;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::director::BootTarget;
use crate::director::Device;
use crate::operating_systems::OperatingSystemsStore;
use crate::roles::RolesStore;
use crate::storage::ImageStore;
use crate::templates;

/// Strongly-typed Action enum representing all possible provisioning actions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    DiscoverHardware,
    ConfigureBmc,
    InstallOs,
    PartitionDisks,
}

/// Context required for converting Actions to BootTargets
pub struct ActionContext<'a> {
    pub root_url: &'a str,
    pub device: &'a Device,
    pub os_store: &'a OperatingSystemsStore,
    pub roles_store: &'a RolesStore,
    pub image_store: &'a Arc<dyn ImageStore>,
}

impl Action {
    /// Convert this Action to a BootTarget for the device
    ///
    /// This method determines what the device should boot into based on the current action.
    /// For most actions, this returns LocalDisk. For special actions like hardware discovery,
    /// BMC configuration, and OS installation, it returns NetBoot with appropriate kernel/initramfs.
    pub async fn to_boot_target(&self, ctx: &ActionContext<'_>) -> Result<BootTarget> {
        match self {
            Action::DiscoverHardware => generate_agent_boot_target(ctx.root_url, "device-scan"),
            Action::ConfigureBmc => generate_agent_boot_target(ctx.root_url, "configure-bmc"),
            Action::InstallOs => generate_os_install_boot_target(ctx).await,
            // All other actions default to local disk boot
            _ => Ok(BootTarget::LocalDisk),
        }
    }
}

/// Generate boot target for agent-based actions (discovery, BMC config, etc.)
fn generate_agent_boot_target(root_url: &str, action_name: &str) -> Result<BootTarget> {
    let kernel_url = format!("{}/cnc/agent-images/vmlinuz", root_url);
    let initramfs_url = format!("{}/cnc/agent-images/initramfs.img", root_url);
    let cmdline = format!(
        "rackdirector.url={}/cnc rackdirector.action={} ro console=ttyS1,115200n8 earlyprintk=serial,ttyS1,115200n8",
        root_url, action_name
    );

    Ok(BootTarget::NetBoot {
        ramdisk: initramfs_url,
        kernel: kernel_url,
        cmdline,
    })
}

/// Generate boot target for OS installation
async fn generate_os_install_boot_target(ctx: &ActionContext<'_>) -> Result<BootTarget> {
    // Get device architecture from the passed device
    let architecture = ctx.device.architecture;

    // Get device role
    let role = ctx
        .roles_store
        .get_device_role(&ctx.device.uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("role not found for {}", ctx.device.uuid))?;

    // Get OS architecture configuration
    let os_arch = ctx
        .os_store
        .get_architecture(role.os_id, architecture)
        .await?;

    // Generate URLs from image store
    let kernel_url = ctx.image_store.get_url(&os_arch.kernel_path);
    let initramfs_url = ctx.image_store.get_url(&os_arch.initramfs_path);

    let cmdline = os_arch
        .cmdline_args
        .map(|template| templates::render_cmdline_args(&template, ctx.root_url))
        .transpose()?
        .unwrap_or_default();

    Ok(BootTarget::NetBoot {
        ramdisk: initramfs_url,
        kernel: kernel_url,
        cmdline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::Device;
    use crate::operating_systems::Architecture;
    use crate::roles::DiskLayout;
    use crate::storage::MemoryImageStore;
    use crate::{database, operating_systems::OperatingSystemsStore, roles::RolesStore};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    /// Helper to create test database and stores for ActionContext
    async fn setup_test_stores() -> (
        Arc<Mutex<rusqlite::Connection>>,
        OperatingSystemsStore,
        RolesStore,
        Arc<dyn ImageStore>,
        tempfile::TempDir,
    ) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Arc::new(Mutex::new(database::open(&db_path).unwrap()));
        let os_store = OperatingSystemsStore::new(db.clone());
        let roles_store = RolesStore::new(db.clone());
        let image_store: Arc<dyn ImageStore> = Arc::new(MemoryImageStore::new());
        (db, os_store, roles_store, image_store, temp_dir)
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
        ];

        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            assert!(json.contains(r#"{"type":"#));
        }
    }

    #[test]
    fn test_generate_agent_boot_target() {
        let boot_target =
            generate_agent_boot_target("http://localhost:8080", "device-scan").unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert_eq!(kernel, "http://localhost:8080/cnc/agent-images/vmlinuz");
                assert_eq!(
                    ramdisk,
                    "http://localhost:8080/cnc/agent-images/initramfs.img"
                );
                assert!(cmdline.contains("rackdirector.url=http://localhost:8080/cnc"));
                assert!(cmdline.contains("rackdirector.action=device-scan"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_discover_hardware_action_to_boot_target() {
        let (_db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        let device = Device {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:8080",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::DiscoverHardware;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert_eq!(kernel, "http://localhost:8080/cnc/agent-images/vmlinuz");
                assert_eq!(
                    ramdisk,
                    "http://localhost:8080/cnc/agent-images/initramfs.img"
                );
                assert!(cmdline.contains("rackdirector.url=http://localhost:8080/cnc"));
                assert!(cmdline.contains("rackdirector.action=device-scan"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot for DiscoverHardware, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_configure_bmc_action_to_boot_target() {
        let (_db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        let device = Device {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:9000",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::ConfigureBmc;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert_eq!(kernel, "http://localhost:9000/cnc/agent-images/vmlinuz");
                assert_eq!(
                    ramdisk,
                    "http://localhost:9000/cnc/agent-images/initramfs.img"
                );
                assert!(cmdline.contains("rackdirector.url=http://localhost:9000/cnc"));
                assert!(cmdline.contains("rackdirector.action=configure-bmc"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot for ConfigureBmc, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_install_os_action_to_boot_target_success() {
        let (db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        // Create an OS with architecture
        let os = os_store
            .create("Ubuntu", "24.04", Some("Ubuntu 24.04 LTS"))
            .await
            .unwrap();
        let os_id = os.id.unwrap();

        // Add architecture configuration with cmdline template
        os_store
            .upsert_architecture(
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
        let disk_layout = DiskLayout { partitions: vec![] };
        let role = roles_store
            .create(
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
        {
            let conn = db.lock().await;
            conn.execute(
                "INSERT INTO devices (uuid, architecture, lifecycle, role_id) VALUES (?1, 'x86-64', 'new', ?2)",
                rusqlite::params![device_uuid, role_id],
            )
            .unwrap();
        }

        let device = Device {
            uuid: device_uuid,
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: Some(role_id),
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:8080",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::InstallOs;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert!(kernel.contains("installer/ubuntu-24.04/vmlinuz"));
                assert!(ramdisk.contains("installer/ubuntu-24.04/initrd.img"));
                assert!(cmdline.contains("console=ttyS0"));
                assert!(cmdline.contains("autoinstall"));
                assert!(cmdline.contains("http://localhost:8080/cnc/install_script"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot for InstallOs, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_install_os_action_missing_role_error() {
        let (_db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        // Create device without a role
        let device = Device {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:8080",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::InstallOs;
        let result = action.to_boot_target(&ctx).await;

        assert!(result.is_err(), "Expected error when device has no role");
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("role not found"),
            "Error message should mention missing role, got: {}",
            error_msg
        );
    }

    #[tokio::test]
    async fn test_install_os_action_with_no_cmdline_template() {
        let (db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        // Create an OS with architecture but NO cmdline template
        let os = os_store
            .create("Debian", "12", Some("Debian 12"))
            .await
            .unwrap();
        let os_id = os.id.unwrap();

        // Add architecture configuration WITHOUT cmdline template
        os_store
            .upsert_architecture(
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
        let disk_layout = DiskLayout { partitions: vec![] };
        let role = roles_store
            .create(
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
        {
            let conn = db.lock().await;
            conn.execute(
                "INSERT INTO devices (uuid, architecture, lifecycle, role_id) VALUES (?1, 'x86-64', 'new', ?2)",
                rusqlite::params![device_uuid, role_id],
            )
            .unwrap();
        }

        let device = Device {
            uuid: device_uuid,
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: Some(role_id),
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:8080",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::InstallOs;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert!(kernel.contains("installer/debian-12/vmlinuz"));
                assert!(ramdisk.contains("installer/debian-12/initrd.img"));
                // cmdline should be empty when no template is provided
                assert_eq!(
                    cmdline, "",
                    "Expected empty cmdline when no template provided"
                );
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot for InstallOs, got LocalDisk"),
        }
    }

    #[tokio::test]
    async fn test_partition_disks_action_returns_local_disk() {
        let (_db, os_store, roles_store, image_store, _temp_dir) = setup_test_stores().await;

        let device = Device {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440006").unwrap(),
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            attributes: common::device_attributes::DeviceAttributes::default(),
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        };

        let ctx = ActionContext {
            root_url: "http://localhost:8080",
            device: &device,
            os_store: &os_store,
            roles_store: &roles_store,
            image_store: &image_store,
        };

        let action = Action::PartitionDisks;
        let boot_target = action.to_boot_target(&ctx).await.unwrap();

        match boot_target {
            BootTarget::LocalDisk => {} // Expected
            BootTarget::NetBoot { .. } => {
                panic!("Expected LocalDisk for PartitionDisks, got NetBoot")
            }
        }
    }
}
