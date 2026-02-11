use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::dhcp::DhcpStore;
use crate::director::store::DirectorStore;
use crate::director::store::generate_hostname_from_uuid;
use crate::lifecycle::{DeviceLifecycle, LifecycleManager, LifecycleStore, LifecycleTransition};
use crate::operating_systems::{Architecture, OperatingSystemsStore};
use crate::plans::actions::BootTarget;
use crate::plans::{Plan, PlanStatus, PlansStore};
use crate::roles::RolesStore;

mod ipmi;
mod store;

pub use common::device_attributes::NetworkInterface;
pub use store::Device;
pub use store::PendingDevice;

#[derive(Clone)]
pub struct Director {
    store: DirectorStore,
    plans_store: PlansStore,
    lifecycle_store: LifecycleStore,
    os_store: OperatingSystemsStore,
    roles_store: RolesStore,
    dhcp_store: DhcpStore,
}

impl Director {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        let store = DirectorStore::new(conn.clone());
        let plans_store = PlansStore::new(conn.clone());
        let lifecycle_store = LifecycleStore::new(conn.clone());
        let os_store = OperatingSystemsStore::new(conn.clone());
        let roles_store = RolesStore::new(conn.clone());
        let dhcp_store = DhcpStore::new(conn);
        Director {
            store,
            plans_store,
            lifecycle_store,
            os_store,
            roles_store,
            dhcp_store,
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

        // Set default BMC config to DHCP
        // Credentials will be auto-generated on first agent fetch
        let default_bmc_config = common::device_attributes::BmcConfig {
            ip_address_source: "dhcp".to_string(),
            ip_address: None,
            netmask: None,
            gateway: None,
            username: None,
            password: None,
        };

        let mut attrs = serde_json::Map::new();
        attrs.insert(
            "bmc_config".to_string(),
            serde_json::to_value(&default_bmc_config)?,
        );
        self.store.update_attributes(uuid, attrs).await?;

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
                device: &device,
                os_store: &self.os_store,
                roles_store: &self.roles_store,
                director: None, // Director not needed for boot target resolution
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

    /// Handle device boot event
    ///
    /// Called when a device boots via iPXE. If the current action's advance_on_boot()
    /// returns true, the action is automatically marked as successful and the plan advances.
    /// This is used for actions like RebootDevice that complete when the device boots.
    pub async fn on_boot(&self, device_uuid: &Uuid) -> anyhow::Result<()> {
        // Get the current active plan
        let mut plan = match self
            .plans_store
            .get_active_plan_for_device(device_uuid)
            .await?
        {
            Some(plan) => plan,
            None => {
                // No active plan - this is normal for devices that have completed their lifecycle
                log::debug!("No active plan for device {} on boot", device_uuid);
                return Ok(());
            }
        };

        // Start the plan if it's pending
        let plan_was_pending = plan.status == PlanStatus::Pending;
        if plan_was_pending {
            plan.start();
        }

        // Check if current action should advance on boot
        let should_advance = if let Some(action) = plan.get_current_action() {
            if action.advance_on_boot() {
                log::info!(
                    "Device {} booted - advancing action {:?}",
                    device_uuid,
                    action
                );

                // Mark current action as successful and advance
                let _result = plan.mark_action_success();
                true
            } else {
                log::debug!(
                    "Device {} booted but current action {:?} does not advance on boot",
                    device_uuid,
                    action
                );
                false
            }
        } else {
            false
        };

        // Update the plan in the database if it was started or advanced
        if plan_was_pending || should_advance {
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
        }

        Ok(())
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

        // Get the newly created plan to access its current action
        let plan = self
            .plans_store
            .get_active_plan_for_device(device_uuid)
            .await?
            .expect("Plan should exist immediately after creation");

        // If there's a current action, run its start() hook
        if let Some(action) = plan.get_current_action() {
            let device = self.get_device(device_uuid).await?;
            let ctx = crate::plans::actions::ActionContext {
                device: &device,
                os_store: &self.os_store,
                roles_store: &self.roles_store,
                director: Some(self), // Provide director for actions that need it (e.g., RebootDevice)
            };

            log::debug!("Starting action {:?} for device {}", action, device_uuid);
            if let Err(e) = action.start(&ctx).await {
                log::warn!(
                    "Failed to execute start() for action {:?} on device {}: {}",
                    action,
                    device_uuid,
                    e
                );
            }
        }

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
        // CRITICAL: Clean up reservations BEFORE deleting device
        // (interfaces stored in device JSON, lost after deletion)

        // 1. Delete for all discovered interfaces
        let interfaces = self.store.get_network_interfaces(uuid).await.unwrap_or_default();
        for nic in &interfaces {
            match self.dhcp_store.delete_static_reservations_by_mac(&nic.mac_address).await {
                Ok(count) if count > 0 => {
                    log::info!(
                        "Deleted {} reservation(s) for MAC {} (device {})",
                        count,
                        nic.mac_address,
                        uuid
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Failed to delete reservations for MAC {}: {}",
                        nic.mac_address,
                        e
                    );
                }
                _ => {}
            }
        }

        // 2. Check legacy mac_address field (backward compatibility)
        if let Ok(device) = self.store.get_device(uuid).await {
            if let Some(mac) = &device.attributes.mac_address {
                if !interfaces.iter().any(|nic| &nic.mac_address == mac) {
                    let _ = self.dhcp_store.delete_static_reservations_by_mac(mac).await;
                }
            }
        }

        // 3. Delete device (cascades to plans, transitions)
        self.store.delete_device(uuid).await
    }

    pub async fn find_device_by_bmc_mac(&self, mac: &str) -> anyhow::Result<Option<Uuid>> {
        self.store.find_device_by_bmc_mac(mac).await
    }

    #[cfg(test)]
    pub fn dhcp_store(&self) -> &DhcpStore {
        &self.dhcp_store
    }

    /// Issue an IPMI power reset command to the device's BMC
    ///
    /// This is a best-effort operation - if the BMC IP or credentials are missing,
    /// or if the IPMI command fails, the error is logged but not propagated.
    /// This allows lifecycle transitions to proceed even if IPMI reboot fails
    /// (the device can be manually rebooted).
    pub async fn reboot(&self, uuid: &Uuid) -> anyhow::Result<()> {
        // Get BMC IP address
        let bmc_ip = match self.get_bmc_ip(uuid).await? {
            Some(ip) => ip,
            None => {
                log::info!("No BMC IP for device {}, skipping power reset", uuid);
                return Ok(());
            }
        };

        // Get BMC credentials
        let (username, password) = match self.get_bmc_credentials(uuid).await? {
            Some(creds) => creds,
            None => {
                log::info!(
                    "No BMC credentials for device {}, skipping power reset",
                    uuid
                );
                return Ok(());
            }
        };

        // Create IPMI client and send power reset
        let ipmi = ipmi::IpmiClient::new(bmc_ip.clone(), username, password);
        match ipmi.power_reset().await {
            Ok(_) => {
                log::info!("IPMI power reset sent to device {} at {}", uuid, bmc_ip);
            }
            Err(e) => {
                log::warn!(
                    "Failed to send IPMI power reset to device {} at {}: {}",
                    uuid,
                    bmc_ip,
                    e
                );
            }
        }

        Ok(())
    }

    /// Get BMC IP address from device attributes
    async fn get_bmc_ip(&self, uuid: &Uuid) -> anyhow::Result<Option<String>> {
        let device = self.store.get_device(uuid).await?;
        Ok(device.attributes.bmc.and_then(|bmc| bmc.ip_address))
    }

    /// Get BMC credentials from device attributes
    async fn get_bmc_credentials(&self, uuid: &Uuid) -> anyhow::Result<Option<(String, String)>> {
        let device = self.store.get_device(uuid).await?;
        let bmc_config = device.attributes.bmc_config;

        match bmc_config {
            Some(config) => match (config.username, config.password) {
                (Some(u), Some(p)) => Ok(Some((u, p))),
                _ => Ok(None),
            },
            None => Ok(None),
        }
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
            BootTarget::AgentImage { action, cmdline: _ } => {
                assert_eq!(action, "device-scan");
            }
            _ => panic!("Expected NetBoot, got LocalDisk"),
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
        assert!(
            matches!(boot_target, BootTarget::AgentImage { action, cmdline } if action == "configure-bmc")
        );

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

    #[tokio::test]
    async fn test_reboot_with_bmc_info() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440020").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC info and credentials
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc".to_string(),
            serde_json::json!({
                "mac_address": "aa:bb:cc:dd:ee:ff",
                "ip_address": "10.0.0.100"
            }),
        );
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "username": "RACKDIRECTOR",
                "password": "test_password"
            }),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Call reboot - should not fail even if ipmitool is not installed
        // (it's best-effort)
        let result = director.reboot(&test_uuid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reboot_without_bmc_ip() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440021").unwrap();

        // Register device without BMC info
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Call reboot - should succeed (gracefully skip IPMI)
        let result = director.reboot(&test_uuid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reboot_without_bmc_credentials() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440022").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC IP but no credentials
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc".to_string(),
            serde_json::json!({
                "mac_address": "aa:bb:cc:dd:ee:ff",
                "ip_address": "10.0.0.100"
            }),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Call reboot - should succeed (gracefully skip IPMI)
        let result = director.reboot(&test_uuid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_on_boot_advances_reboot_action() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440023").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a plan with RebootDevice and InstallOs actions
        let actions = vec![
            crate::plans::Action::RebootDevice,
            crate::plans::Action::InstallOs,
        ];
        let plan = crate::plans::Plan::new(test_uuid, actions);
        director.create_plan(&plan).await.unwrap();

        // Verify plan is active with RebootDevice as current action
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 0);
        assert_eq!(plan.actions[0], crate::plans::Action::RebootDevice);

        // Call on_boot - should advance past RebootDevice
        director.on_boot(&test_uuid).await.unwrap();

        // Verify plan advanced to next action
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 1);
        assert_eq!(plan.actions[1], crate::plans::Action::InstallOs);
    }

    #[tokio::test]
    async fn test_on_boot_does_not_advance_other_actions() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440024").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a plan with DiscoverHardware action
        let actions = vec![crate::plans::Action::DiscoverHardware];
        let plan = crate::plans::Plan::new(test_uuid, actions);
        director.create_plan(&plan).await.unwrap();

        // Verify plan is initially Pending
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        assert_eq!(active_plan.unwrap().status, PlanStatus::Pending);

        // Call on_boot - should start the plan but NOT advance (DiscoverHardware doesn't advance on boot)
        director.on_boot(&test_uuid).await.unwrap();

        // Verify plan is now Running but did not advance to next step
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 0);
        assert_eq!(plan.status, PlanStatus::Running);
    }

    #[tokio::test]
    async fn test_on_boot_without_active_plan() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440025").unwrap();

        // Register device without a plan
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Call on_boot - should succeed without error
        let result = director.on_boot(&test_uuid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_bmc_ip_with_valid_bmc() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440026").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC IP
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc".to_string(),
            serde_json::json!({
                "mac_address": "aa:bb:cc:dd:ee:ff",
                "ip_address": "10.0.0.100"
            }),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Get BMC IP
        let bmc_ip = director.get_bmc_ip(&test_uuid).await.unwrap();
        assert_eq!(bmc_ip, Some("10.0.0.100".to_string()));
    }

    #[tokio::test]
    async fn test_get_bmc_ip_without_bmc() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440027").unwrap();

        // Register device without BMC
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Get BMC IP
        let bmc_ip = director.get_bmc_ip(&test_uuid).await.unwrap();
        assert_eq!(bmc_ip, None);
    }

    #[tokio::test]
    async fn test_get_bmc_credentials_with_valid_config() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440028").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC credentials
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "dhcp",
                "username": "RACKDIRECTOR",
                "password": "test_password"
            }),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Get BMC credentials
        let creds = director.get_bmc_credentials(&test_uuid).await.unwrap();
        assert_eq!(
            creds,
            Some(("RACKDIRECTOR".to_string(), "test_password".to_string()))
        );
    }

    #[tokio::test]
    async fn test_get_bmc_credentials_without_credentials() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440029").unwrap();

        // Register device - will have default BMC config with no credentials
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Get BMC credentials - should be None even though config exists
        let creds = director.get_bmc_credentials(&test_uuid).await.unwrap();
        assert_eq!(creds, None);
    }

    #[tokio::test]
    async fn test_register_device_sets_default_bmc_config() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440030").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Get device and verify default BMC config
        let device = director.get_device(&test_uuid).await.unwrap();

        // Should have BMC config
        assert!(device.attributes.bmc_config.is_some());

        let bmc_config = device.attributes.bmc_config.unwrap();

        // Should be set to DHCP by default
        assert_eq!(bmc_config.ip_address_source, "dhcp");

        // All other fields should be None
        assert_eq!(bmc_config.ip_address, None);
        assert_eq!(bmc_config.netmask, None);
        assert_eq!(bmc_config.gateway, None);
        assert_eq!(bmc_config.username, None);
        assert_eq!(bmc_config.password, None);
    }

    #[tokio::test]
    async fn test_default_bmc_config_does_not_override_existing() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440031").unwrap();

        // Register device (sets default config)
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Manually set a custom BMC config
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "bmc_config".to_string(),
            serde_json::json!({
                "ip_address_source": "static",
                "ip_address": "10.0.0.100",
                "netmask": "255.255.255.0",
                "gateway": "10.0.0.1",
                "username": "CUSTOM",
                "password": "custom_pass"
            }),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Verify custom config is preserved
        let device = director.get_device(&test_uuid).await.unwrap();
        let bmc_config = device.attributes.bmc_config.unwrap();

        assert_eq!(bmc_config.ip_address_source, "static");
        assert_eq!(bmc_config.ip_address, Some("10.0.0.100".parse().unwrap()));
        assert_eq!(bmc_config.netmask, Some("255.255.255.0".parse().unwrap()));
        assert_eq!(bmc_config.gateway, Some("10.0.0.1".parse().unwrap()));
        assert_eq!(bmc_config.username, Some("CUSTOM".to_string()));
        assert_eq!(bmc_config.password, Some("custom_pass".to_string()));
    }

    #[tokio::test]
    async fn test_default_bmc_config_preserves_other_attributes() {
        let (director, _temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440032").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Verify hostname was set (from register_device)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "node-446655440032"
        );

        // Verify BMC config was also set
        assert!(device.attributes.bmc_config.is_some());

        // Add additional attributes
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "manufacturer".to_string(),
            serde_json::Value::String("Dell Inc.".to_string()),
        );
        director
            .update_attributes(&test_uuid, attributes)
            .await
            .unwrap();

        // Verify all attributes are preserved
        let device = director.get_device(&test_uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "node-446655440032"
        );
        assert!(device.attributes.bmc_config.is_some());
        assert_eq!(
            device.attributes.manufacturer.as_ref().unwrap(),
            "Dell Inc."
        );
    }

    #[tokio::test]
    async fn test_delete_device_removes_static_reservations() {
        let (director, temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440033").unwrap();

        // Create a DHCP network first
        let db_path = temp_dir.path().join("test.db");
        let db = crate::database::open(&db_path).unwrap();
        let db_tokio = Arc::new(Mutex::new(db));
        let dhcp_store = crate::dhcp::DhcpStore::new(db_tokio);

        let network = dhcp_store
            .create_network(
                "Test Network",
                "10.0.0.0/24",
                "10.0.0.1",
                &["8.8.8.8".to_string()],
                86400,
                None,
                false,
            )
            .await
            .unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create network interface with MAC
        let mac = "aa:bb:cc:dd:ee:33";
        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network.id),
            disabled: false,
            warning_label: None,
        }];
        director
            .set_network_interfaces(&test_uuid, &interfaces)
            .await
            .unwrap();

        // Create static reservation
        dhcp_store
            .create_or_update_static_reservation(network.id, mac, "10.0.0.100", None)
            .await
            .unwrap();

        // Verify reservation exists
        let reservation = dhcp_store
            .get_static_reservation(network.id, mac)
            .await
            .unwrap();
        assert!(reservation.is_some());

        // Delete device
        director.delete_device(&test_uuid).await.unwrap();

        // Verify reservation was deleted
        let reservation = dhcp_store
            .get_static_reservation(network.id, mac)
            .await
            .unwrap();
        assert!(reservation.is_none());
    }

    #[tokio::test]
    async fn test_delete_device_removes_reservations_for_multiple_nics() {
        let (director, temp_dir) = setup_test_director().await;
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440034").unwrap();

        // Create DHCP network
        let db_path = temp_dir.path().join("test.db");
        let db = crate::database::open(&db_path).unwrap();
        let db_tokio = Arc::new(Mutex::new(db));
        let dhcp_store = crate::dhcp::DhcpStore::new(db_tokio);

        let network = dhcp_store
            .create_network(
                "Test Network",
                "10.0.0.0/24",
                "10.0.0.1",
                &["8.8.8.8".to_string()],
                86400,
                None,
                false,
            )
            .await
            .unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create multiple network interfaces
        let mac1 = "aa:bb:cc:dd:ee:34";
        let mac2 = "aa:bb:cc:dd:ee:35";
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac1.to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: Some(network.id),
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: mac2.to_string(),
                ip_address: Some("10.0.0.102".to_string()),
                network_id: Some(network.id),
                disabled: false,
                warning_label: None,
            },
        ];
        director
            .set_network_interfaces(&test_uuid, &interfaces)
            .await
            .unwrap();

        // Create static reservations for both MACs
        dhcp_store
            .create_or_update_static_reservation(network.id, mac1, "10.0.0.101", None)
            .await
            .unwrap();
        dhcp_store
            .create_or_update_static_reservation(network.id, mac2, "10.0.0.102", None)
            .await
            .unwrap();

        // Verify both reservations exist
        assert!(dhcp_store
            .get_static_reservation(network.id, mac1)
            .await
            .unwrap()
            .is_some());
        assert!(dhcp_store
            .get_static_reservation(network.id, mac2)
            .await
            .unwrap()
            .is_some());

        // Delete device
        director.delete_device(&test_uuid).await.unwrap();

        // Verify both reservations were deleted
        assert!(dhcp_store
            .get_static_reservation(network.id, mac1)
            .await
            .unwrap()
            .is_none());
        assert!(dhcp_store
            .get_static_reservation(network.id, mac2)
            .await
            .unwrap()
            .is_none());
    }
}
