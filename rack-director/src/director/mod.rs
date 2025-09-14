use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;

use crate::director::store::DirectorStore;
use crate::plans::{Plan, PlanStatus, PlansStore};
use crate::tftp::Handler;
use crate::tftp::Reader;

mod store;

pub enum BootTarget {
    LocalDisk,
    NetBoot {
        ramdisk: String,
        kernel: String,
        cmdline: String,
    },
}

#[derive(Clone)]
pub struct Director {
    store: DirectorStore,
    plans_store: PlansStore,
}

impl Director {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        let store = DirectorStore::new(conn.clone());
        let plans_store = PlansStore::new(conn);
        Director { store, plans_store }
    }

    pub async fn register_device(&self, uuid: &str) -> anyhow::Result<()> {
        self.store.register_device(uuid).await?;

        Ok(())
    }

    pub async fn next_boot_target(&self, uuid: &str) -> anyhow::Result<BootTarget> {
        self.store
            .update_device_last_seen(uuid)
            .await
            .expect("update device last seen should not fail");

        // Check if there's an active plan for this device
        if let Some(plan) = self.plans_store.get_active_plan_for_device(uuid).await? {
            if let Some(current_action) = plan.get_current_action() {
                // Return appropriate boot target based on the current action
                return Ok(self.get_boot_target_for_action(current_action));
            }
        }

        // Default to local disk if no active plan
        Ok(BootTarget::LocalDisk)
    }

    fn get_boot_target_for_action(&self, action: &crate::plans::Action) -> BootTarget {
        match action.action_type.as_str() {
            "install_os" => BootTarget::NetBoot {
                ramdisk: "install-initrd.img".to_string(),
                kernel: "install-vmlinuz".to_string(),
                cmdline: "install".to_string(),
            },
            "configure_network" => BootTarget::NetBoot {
                ramdisk: "config-initrd.img".to_string(),
                kernel: "config-vmlinuz".to_string(),
                cmdline: "configure".to_string(),
            },
            "run_diagnostics" => BootTarget::NetBoot {
                ramdisk: "diag-initrd.img".to_string(),
                kernel: "diag-vmlinuz".to_string(),
                cmdline: "diagnostics".to_string(),
            },
            // Default to local boot for unknown actions
            _ => BootTarget::LocalDisk,
        }
    }

    pub async fn update_attributes(
        &self,
        uuid: &str,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        self.store.update_attributes(uuid, attributes).await?;
        Ok(())
    }

    pub async fn create_plan(&self, plan: &Plan) -> anyhow::Result<i64> {
        self.plans_store.create_plan(plan).await
    }

    pub async fn get_active_plan_for_device(
        &self,
        device_uuid: &str,
    ) -> anyhow::Result<Option<Plan>> {
        self.plans_store
            .get_active_plan_for_device(device_uuid)
            .await
    }

    pub async fn mark_action_success(&self, device_uuid: &str) -> anyhow::Result<()> {
        // Get the current active plan
        let mut plan = match self
            .plans_store
            .get_active_plan_for_device(device_uuid)
            .await?
        {
            Some(plan) => plan,
            None => {
                return Err(anyhow::anyhow!(
                    "No active plan found for device {}",
                    device_uuid
                ));
            }
        };

        // Start the plan if it's pending
        if plan.status == PlanStatus::Pending {
            plan.start();
        }

        // Mark current action as successful and advance
        let _result = plan.mark_action_success();

        // Update the plan in the database
        self.plans_store
            .update_plan_status(
                plan.id.unwrap(),
                plan.status.clone(),
                plan.current_step,
                plan.error_message.as_deref(),
            )
            .await?;

        Ok(())
    }

    pub async fn mark_action_failed(
        &self,
        device_uuid: &str,
        error_message: &str,
    ) -> anyhow::Result<()> {
        // Get the current active plan
        let mut plan = match self
            .plans_store
            .get_active_plan_for_device(device_uuid)
            .await?
        {
            Some(plan) => plan,
            None => {
                return Err(anyhow::anyhow!(
                    "No active plan found for device {}",
                    device_uuid
                ));
            }
        };

        // Mark current action as failed
        let _result = plan.mark_action_failed(error_message.to_string());

        // Update the plan in the database
        self.plans_store
            .update_plan_status(
                plan.id.unwrap(),
                plan.status.clone(),
                plan.current_step,
                plan.error_message.as_deref(),
            )
            .await?;

        Ok(())
    }

    pub async fn get_all_devices(
        &self,
    ) -> anyhow::Result<Vec<(String, Option<serde_json::Map<String, serde_json::Value>>)>> {
        self.store
            .get_all_devices()
            .await
            .map_err(anyhow::Error::from)
    }
}

pub struct DirectorTftpHandler {
    root: PathBuf,
}

impl DirectorTftpHandler {
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        DirectorTftpHandler { root: root.into() }
    }
}

impl Handler for DirectorTftpHandler {
    type Reader = DirectorTftpReader;

    async fn create_reader(&self, filename: &str) -> anyhow::Result<Self::Reader> {
        match filename {
            "ipxe.efi" | "undionly.kpxe" => {
                let reader = DirectorTftpReader::open(&self.root.join(filename)).await?;
                Ok(reader)
            }
            _ => Err(anyhow::anyhow!("Unsupported file: {}", filename)),
        }
    }
}

pub struct DirectorTftpReader {
    file: BufReader<tokio::fs::File>,
}

impl DirectorTftpReader {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let file = tokio::fs::File::open(path).await?;
        Ok(DirectorTftpReader {
            file: BufReader::new(file),
        })
    }
}

impl Reader for DirectorTftpReader {
    async fn read(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut chunk = vec![0; 512]; // Read in chunks of 512 bytes
        let _ = self.file.read(&mut chunk).await?;
        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, plans::PlanStatus};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn setup_test_director() -> (Director, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let director = Director::new(Arc::new(Mutex::new(db)));
        (director, temp_dir)
    }

    #[tokio::test]
    async fn test_single_active_plan_constraint() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = "550e8400-e29b-41d4-a716-446655440006";

        // Register device
        director.register_device(test_uuid).await.unwrap();

        // Create first plan
        let first_actions = vec![crate::plans::Action::new(
            "install_os".to_string(),
            std::collections::HashMap::new(),
        )];
        let first_plan = crate::plans::Plan::new(test_uuid.to_string(), first_actions);
        director.create_plan(&first_plan).await.unwrap();

        // Verify first plan is active
        let active_plan = director
            .get_active_plan_for_device(test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        assert_eq!(
            active_plan.as_ref().unwrap().actions[0].action_type,
            "install_os"
        );

        // Create second plan - this should be rejected
        let second_actions = vec![crate::plans::Action::new(
            "configure_network".to_string(),
            std::collections::HashMap::new(),
        )];
        let second_plan = crate::plans::Plan::new(test_uuid.to_string(), second_actions);
        let result = director.create_plan(&second_plan).await;

        // Verify the second plan creation was rejected
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("an active plan already exists")
        );

        // Verify the first plan is still active and unchanged
        let active_plan = director
            .get_active_plan_for_device(test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.actions[0].action_type, "install_os");
        assert_eq!(plan.status, PlanStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_all_devices() {
        let (director, _temp_dir) = setup_test_director().await;
        
        // Initially should return empty list
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 0);
        
        // Register a device
        let test_uuid1 = "550e8400-e29b-41d4-a716-446655440001";
        director.register_device(test_uuid1).await.unwrap();
        
        // Should now return one device
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].0, test_uuid1);
        // Default attributes should be empty JSON object
        let attrs = devices[0].1.as_ref().unwrap();
        assert!(attrs.is_empty());
        
        // Register another device with attributes
        let test_uuid2 = "550e8400-e29b-41d4-a716-446655440002";
        director.register_device(test_uuid2).await.unwrap();
        
        let mut attributes = serde_json::Map::new();
        attributes.insert("hostname".to_string(), serde_json::Value::String("test-server".to_string()));
        director.update_attributes(test_uuid2, attributes.clone()).await.unwrap();
        
        // Should now return two devices
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 2);
        
        // Find the device with attributes
        let device_with_attrs = devices.iter()
            .find(|(uuid, _)| uuid == test_uuid2)
            .unwrap();
        assert!(device_with_attrs.1.is_some());
        let attrs = device_with_attrs.1.as_ref().unwrap();
        assert_eq!(attrs.get("hostname").unwrap().as_str().unwrap(), "test-server");
    }
}
