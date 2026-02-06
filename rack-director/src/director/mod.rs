use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::director::store::DirectorStore;
use crate::director::store::generate_hostname_from_uuid;
use crate::lifecycle::{DeviceLifecycle, LifecycleManager, LifecycleStore, LifecycleTransition};
use crate::operating_systems::{Architecture, OperatingSystemsStore};
use crate::plans::{Plan, PlanStatus, PlansStore};
use crate::roles::RolesStore;
use crate::storage::ImageStore;

mod store;

pub use common::device_attributes::NetworkInterface;
pub use store::Device;
pub use store::PendingDevice;

#[derive(Debug)]
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
    lifecycle_store: LifecycleStore,
    os_store: OperatingSystemsStore,
    roles_store: RolesStore,
    image_store: Arc<dyn ImageStore>,
    root_url: String,
}

impl Director {
    pub fn new<T: Into<String>>(
        conn: Arc<Mutex<rusqlite::Connection>>,
        image_store: Arc<dyn ImageStore>,
        root_url: T,
    ) -> Self {
        let store = DirectorStore::new(conn.clone());
        let plans_store = PlansStore::new(conn.clone());
        let lifecycle_store = LifecycleStore::new(conn.clone());
        let os_store = OperatingSystemsStore::new(conn.clone());
        let roles_store = RolesStore::new(conn);
        Director {
            store,
            plans_store,
            lifecycle_store,
            os_store,
            roles_store,
            image_store,
            root_url: root_url.into(),
        }
    }

    pub async fn register_device(
        &self,
        uuid: &Uuid,
        architecture: Architecture,
    ) -> anyhow::Result<()> {
        log::info!("Registering device {uuid}");
        self.store.register_device(uuid, architecture).await?;
        self.store
            .set_hostname(uuid, &generate_hostname_from_uuid(uuid))
            .await?;
        Ok(())
    }

    pub async fn device_exists(&self, uuid: &Uuid) -> anyhow::Result<bool> {
        let exists = self.store.device_exists(uuid).await?;
        Ok(exists)
    }

    pub async fn next_boot_target(&self, uuid: &Uuid) -> anyhow::Result<BootTarget> {
        self.store
            .update_device_last_seen(uuid)
            .await
            .expect("update device last seen should not fail");

        // Check if there's an active plan for this device
        if let Some(plan) = self.plans_store.get_active_plan_for_device(uuid).await?
            && let Some(current_action) = plan.get_current_action()
        {
            // Get device for ActionContext
            let device = self.get_device(uuid).await?;

            // Create ActionContext for the action
            let ctx = crate::plans::actions::ActionContext {
                root_url: &self.root_url,
                device: &device,
                os_store: &self.os_store,
                roles_store: &self.roles_store,
                image_store: &self.image_store,
            };

            // Return appropriate boot target based on the current action
            return current_action.to_boot_target(&ctx).await;
        }

        // Default to local disk if no active plan
        Ok(BootTarget::LocalDisk)
    }

    pub async fn update_attributes(
        &self,
        uuid: &Uuid,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        self.store.update_attributes(uuid, attributes).await?;
        Ok(())
    }

    pub async fn create_plan(&self, plan: &Plan) -> anyhow::Result<i64> {
        self.plans_store.create_plan(plan).await
    }

    #[cfg(test)]
    pub async fn get_active_plan_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> anyhow::Result<Option<Plan>> {
        self.plans_store
            .get_active_plan_for_device(device_uuid)
            .await
    }

    pub async fn mark_action_success(&self, device_uuid: &Uuid) -> anyhow::Result<()> {
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

        // Handle lifecycle transition if plan is complete
        if plan.status == PlanStatus::Success {
            self.handle_plan_completion_success(plan.id.unwrap())
                .await?;
        }

        Ok(())
    }

    pub async fn mark_action_failed(
        &self,
        device_uuid: &Uuid,
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

        // Handle lifecycle transition if plan failed
        self.handle_plan_completion_failure(plan.id.unwrap(), error_message)
            .await?;

        Ok(())
    }

    pub async fn start_lifecycle_transition(
        &self,
        device_uuid: &Uuid,
        to_state: DeviceLifecycle,
    ) -> anyhow::Result<i64> {
        // Get current device lifecycle
        let current_lifecycle = self
            .lifecycle_store
            .get_device_lifecycle(device_uuid)
            .await?
            .unwrap_or(DeviceLifecycle::New);

        // Check if transition is allowed
        if !LifecycleManager::is_transition_allowed(&current_lifecycle, &to_state) {
            return Err(anyhow::anyhow!(
                "Transition from {:?} to {:?} is not allowed",
                current_lifecycle,
                to_state
            ));
        }

        // Check if there's already an active transition
        if let Some(_active_transition) = self
            .lifecycle_store
            .get_active_transition_for_device(device_uuid)
            .await?
        {
            return Err(anyhow::anyhow!(
                "Device {} already has an active lifecycle transition",
                device_uuid
            ));
        }

        // Get transition type
        let transition_type = LifecycleManager::get_transition_type(&current_lifecycle, &to_state)
            .ok_or_else(|| anyhow::anyhow!("Cannot determine transition type"))?;

        // Create plan for this transition
        let actions = LifecycleManager::get_plan_stub_for_transition(&transition_type);
        let plan = Plan::new(*device_uuid, actions);
        let plan_id = self.create_plan(&plan).await?;

        // Create lifecycle transition
        let transition =
            LifecycleTransition::new(*device_uuid, current_lifecycle, to_state, Some(plan_id));

        let transition_id = self.lifecycle_store.create_transition(&transition).await?;

        Ok(transition_id)
    }

    pub async fn get_device_lifecycle(
        &self,
        device_uuid: &Uuid,
    ) -> anyhow::Result<Option<DeviceLifecycle>> {
        self.lifecycle_store.get_device_lifecycle(device_uuid).await
    }

    pub async fn get_active_transition_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> anyhow::Result<Option<LifecycleTransition>> {
        self.lifecycle_store
            .get_active_transition_for_device(device_uuid)
            .await
    }

    pub async fn get_device_transitions(
        &self,
        device_uuid: &Uuid,
        include_completed: bool,
    ) -> anyhow::Result<Vec<LifecycleTransition>> {
        self.lifecycle_store
            .get_transitions_for_device(device_uuid, include_completed)
            .await
    }

    async fn handle_plan_completion_success(&self, plan_id: i64) -> anyhow::Result<()> {
        // Find the lifecycle transition associated with this plan
        if let Some(transition) = self
            .lifecycle_store
            .get_transition_by_plan_id(plan_id)
            .await?
        {
            // Update device lifecycle to the target state
            self.lifecycle_store
                .update_device_lifecycle(&transition.device_uuid, transition.to_state.clone())
                .await?;

            // Complete the transition successfully
            self.lifecycle_store
                .complete_transition(transition.id.unwrap(), true, None)
                .await?;
        }

        Ok(())
    }

    async fn handle_plan_completion_failure(
        &self,
        plan_id: i64,
        error_message: &str,
    ) -> anyhow::Result<()> {
        // Find the lifecycle transition associated with this plan
        if let Some(transition) = self
            .lifecycle_store
            .get_transition_by_plan_id(plan_id)
            .await?
        {
            // Move device to broken state on failure
            self.lifecycle_store
                .update_device_lifecycle(&transition.device_uuid, DeviceLifecycle::Broken)
                .await?;

            // Complete the transition with failure
            self.lifecycle_store
                .complete_transition(transition.id.unwrap(), false, Some(error_message))
                .await?;
        }

        Ok(())
    }

    pub async fn get_device(&self, uuid: &Uuid) -> anyhow::Result<Device> {
        self.store.get_device(uuid).await
    }

    pub async fn get_all_devices(&self) -> anyhow::Result<Vec<Device>> {
        self.store.get_all_devices().await
    }

    pub async fn find_device_by_mac(&self, mac: &str) -> anyhow::Result<Option<Uuid>> {
        self.store.find_device_by_mac(mac).await
    }

    pub async fn set_device_mac_address(&self, uuid: &Uuid, mac: &str) -> anyhow::Result<()> {
        self.store.set_mac_address(uuid, mac).await
    }

    pub async fn set_device_ip_address(
        &self,
        uuid: &Uuid,
        ip: &str,
        mac: &str,
    ) -> anyhow::Result<()> {
        self.store.set_ip_address(uuid, ip, mac).await
    }

    pub async fn get_network_interfaces(
        &self,
        uuid: &Uuid,
    ) -> anyhow::Result<Vec<NetworkInterface>> {
        self.store.get_network_interfaces(uuid).await
    }

    pub async fn set_network_interfaces(
        &self,
        uuid: &Uuid,
        interfaces: &[NetworkInterface],
    ) -> anyhow::Result<()> {
        self.store.set_network_interfaces(uuid, interfaces).await
    }

    pub async fn find_duplicate_macs_on_network(
        &self,
        mac: &str,
        network_id: i64,
        exclude_device: &Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String)>> {
        self.store
            .find_duplicate_macs_on_network(mac, network_id, exclude_device)
            .await
    }

    pub async fn create_pending_device(
        &self,
        mac_address: &str,
        network_id: i64,
    ) -> anyhow::Result<i64> {
        self.store
            .create_pending_device(mac_address, network_id)
            .await
    }

    pub async fn find_pending_device_by_mac(
        &self,
        mac_address: &str,
    ) -> anyhow::Result<Option<i64>> {
        self.store.find_pending_device_by_mac(mac_address).await
    }

    pub async fn complete_pending_device(
        &self,
        mac_address: &str,
        device_uuid: &Uuid,
    ) -> anyhow::Result<()> {
        self.store
            .complete_pending_device(mac_address, device_uuid)
            .await
    }

    pub async fn get_pending_devices(&self) -> anyhow::Result<Vec<PendingDevice>> {
        self.store.get_pending_devices().await
    }

    pub async fn delete_pending_device(&self, id: i64) -> anyhow::Result<()> {
        self.store.delete_pending_device(id).await
    }

    pub async fn delete_device(&self, uuid: &Uuid) -> anyhow::Result<()> {
        self.store.delete_device(uuid).await
    }

    pub async fn find_device_by_bmc_mac(&self, mac: &str) -> anyhow::Result<Option<Uuid>> {
        self.store.find_device_by_bmc_mac(mac).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, plans::PlanStatus, storage::MemoryImageStore};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn setup_test_director() -> (Director, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let director = Director::new(
            Arc::new(Mutex::new(db)),
            Arc::new(MemoryImageStore::new()),
            "http://localhost:0",
        );
        (director, temp_dir)
    }

    #[tokio::test]
    async fn test_single_active_plan_constraint() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440006").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create first plan
        let first_actions = vec![crate::plans::Action::InstallOs];
        let first_plan = crate::plans::Plan::new(test_uuid, first_actions);
        director.create_plan(&first_plan).await.unwrap();

        // Verify first plan is active
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        assert_eq!(
            active_plan.as_ref().unwrap().actions[0],
            crate::plans::Action::InstallOs
        );

        // Create second plan - this should be rejected
        let second_actions = vec![crate::plans::Action::PartitionDisks];
        let second_plan = crate::plans::Plan::new(test_uuid, second_actions);
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
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.actions[0], crate::plans::Action::InstallOs);
        assert_eq!(plan.status, PlanStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_all_devices() {
        let (director, _temp_dir) = setup_test_director().await;

        // Initially should return empty list
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 0);

        // Register a device
        let test_uuid1 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        director
            .register_device(&test_uuid1, Architecture::X86_64)
            .await
            .unwrap();

        // Should now return one device
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].uuid, test_uuid1);
        assert_eq!(
            devices[0].attributes.hostname.as_ref().unwrap(),
            "node-446655440001"
        );

        // Register another device with attributes
        let test_uuid2 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();
        director
            .register_device(&test_uuid2, Architecture::X86_64)
            .await
            .unwrap();

        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "hostname".to_string(),
            serde_json::Value::String("test-server".to_string()),
        );
        director
            .update_attributes(&test_uuid2, attributes.clone())
            .await
            .unwrap();

        // Should now return two devices
        let devices = director.get_all_devices().await.unwrap();
        assert_eq!(devices.len(), 2);

        // Find the device with attributes
        let device_with_attrs = devices.iter().find(|d| d.uuid == test_uuid2).unwrap();
        assert_eq!(
            device_with_attrs.attributes.hostname.as_ref().unwrap(),
            "test-server"
        );
    }

    #[tokio::test]
    async fn test_discovery_transition() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440007").unwrap();

        // Register device - it should start in "new" state
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        let lifecycle = director.get_device_lifecycle(&test_uuid).await.unwrap();
        assert_eq!(lifecycle, Some(DeviceLifecycle::New));

        // Start discovery transition (New -> Unprovisioned)
        let transition_id = director
            .start_lifecycle_transition(&test_uuid, DeviceLifecycle::Unprovisioned)
            .await
            .unwrap();

        assert!(transition_id > 0);

        // Verify the transition was created
        let active_transition = director
            .get_active_transition_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_transition.is_some());
        let transition = active_transition.unwrap();
        assert_eq!(transition.from_state, DeviceLifecycle::New);
        assert_eq!(transition.to_state, DeviceLifecycle::Unprovisioned);

        // Verify a discovery plan was created with 2 actions
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0], crate::plans::Action::DiscoverHardware);
        assert_eq!(plan.actions[1], crate::plans::Action::ConfigureBmc);

        // Verify the device gets the right boot target for first action (discover_hardware)
        let boot_target = director.next_boot_target(&test_uuid).await.unwrap();
        match boot_target {
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                cmdline,
            } => {
                assert!(kernel.contains("/cnc/agent-images/vmlinuz"));
                assert!(ramdisk.contains("/cnc/agent-images/initramfs.img"));
                assert!(cmdline.contains("rackdirector.url="));
                assert!(cmdline.contains("device-scan"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot, got LocalDisk"),
        }

        // Simulate discovery action completion
        director.mark_action_success(&test_uuid).await.unwrap();

        // Verify second action (configure_bmc) is now current
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 1);

        // Verify the device gets BMC config boot target for second action
        let boot_target = director.next_boot_target(&test_uuid).await.unwrap();
        match boot_target {
            BootTarget::NetBoot {
                ramdisk: _,
                kernel: _,
                cmdline,
            } => {
                assert!(cmdline.contains("configure-bmc"));
            }
            BootTarget::LocalDisk => panic!("Expected NetBoot, got LocalDisk"),
        }

        // Simulate BMC configuration completion
        director.mark_action_success(&test_uuid).await.unwrap();

        // Verify plan is now complete
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_none(), "Plan should be complete");

        // Verify device transitioned to Unprovisioned
        let lifecycle = director.get_device_lifecycle(&test_uuid).await.unwrap();
        assert_eq!(lifecycle, Some(DeviceLifecycle::Unprovisioned));

        // Verify transition is marked as successful
        let transitions = director
            .get_device_transitions(&test_uuid, true)
            .await
            .unwrap();
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].success, Some(true));

        // After discovery, device should boot to local disk
        let boot_target = director.next_boot_target(&test_uuid).await.unwrap();
        match boot_target {
            BootTarget::LocalDisk => {} // Expected
            _ => panic!("Expected LocalDisk after discovery completion"),
        }
    }
}
