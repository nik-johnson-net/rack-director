use uuid::Uuid;

use crate::database::Connection;
use crate::director::store::generate_hostname_from_uuid;
use crate::lifecycle::{DeviceLifecycle, LifecycleManager, LifecycleTransition};
use crate::plans::actions::BootTarget;
use crate::plans::{Plan, PlanStatus};
use crate::{platforms, roles};

mod ipmi;
pub(crate) mod store;

pub use common::device_attributes::NetworkInterface;
pub use store::Device;
pub use store::PendingDevice;

/// Supported CPU architectures for devices managed by rack-director.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Architecture {
    #[serde(rename = "x86-64")]
    X86_64,
}

impl Architecture {
    pub fn as_str(&self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86-64",
        }
    }

    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "x86-64" => Ok(Architecture::X86_64),
            _ => Err(anyhow::anyhow!("Unknown architecture: {}", s)),
        }
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Short-lived handle for executing device management operations against an open database
/// connection.
///
/// `Director<'a>` borrows a `Connection` for its lifetime and uses it directly for all
/// database access. It is constructed at the start of a request or packet handler and
/// dropped at the end; it never opens a new connection itself.
pub struct Director<'a> {
    conn: &'a Connection,
}

impl<'a> Director<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Director { conn }
    }

    pub async fn register_device(
        &self,
        uuid: &Uuid,
        architecture: Architecture,
    ) -> anyhow::Result<()> {
        log::info!("Registering device {uuid}");
        store::register_device(self.conn, uuid, architecture).await?;
        store::set_hostname(self.conn, uuid, &generate_hostname_from_uuid(uuid)).await?;

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
        store::update_attributes(self.conn, uuid, attrs).await?;

        Ok(())
    }

    pub async fn device_exists(&self, uuid: &Uuid) -> anyhow::Result<bool> {
        store::device_exists(self.conn, uuid).await
    }

    /// Get the boot target for this device.
    ///
    /// `sleep_secs` controls how long an unprovisioned or unknown device sleeps
    /// before rebooting to retry PXE boot.  Production callers pass 600; e2e
    /// tests may pass 0 to avoid waiting.
    pub async fn next_boot_target(
        &self,
        uuid: &Uuid,
        sleep_secs: u64,
    ) -> anyhow::Result<BootTarget> {
        if self.device_exists(uuid).await? {
            store::update_device_last_seen(self.conn, uuid)
                .await
                .expect("update device last seen should not fail");

            // Check if there's an active plan for this device
            if let Some(plan) =
                crate::plans::store::get_active_plan_for_device(self.conn, uuid).await?
                && let Some(current_action) = plan.get_current_action()
            {
                // Get device for ActionContext
                let device = store::get_device(self.conn, uuid).await?;

                // Create ActionContext for the action
                let ctx = crate::plans::actions::ActionContext {
                    device: &device,
                    conn: self.conn,
                    director: None, // Director not needed for boot target resolution
                };

                // Return appropriate boot target based on the current action
                return current_action.to_boot_target(&ctx).await;
            }

            // No active plan — only boot local disk if the device is fully provisioned.
            // Any other lifecycle state means the device has no OS yet, so sleep and retry
            // so it will pick up a plan when one becomes available.
            let lifecycle = crate::lifecycle::store::get_device_lifecycle(self.conn, uuid).await?;
            if matches!(lifecycle, Some(DeviceLifecycle::Provisioned)) {
                return Ok(BootTarget::LocalDisk);
            }
        }

        // Unknown device or non-provisioned device with no active plan — sleep and retry.
        Ok(BootTarget::SleepReboot {
            seconds: sleep_secs,
        })
    }

    pub async fn update_attributes(
        &self,
        uuid: &Uuid,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        // Check if this update includes hardware discovery data
        let contains_hardware_info = attributes.contains_key("disks")
            || attributes.contains_key("cpus")
            || attributes.contains_key("memory")
            || attributes.contains_key("network_interfaces");

        // Update the attributes
        store::update_attributes(self.conn, uuid, attributes).await?;

        // Auto-detect platform after hardware discovery data is received
        if contains_hardware_info && let Err(e) = self.auto_detect_platform(uuid).await {
            log::warn!("Failed to auto-detect platform for device {}: {}", uuid, e);
            // Add warning for operator visibility
            self.add_device_warning(
                uuid,
                "Platform auto-detection failed: no matching platform found",
            )
            .await?;
            // Don't fail the update - platform detection is best-effort
        }

        Ok(())
    }

    /// Overwrite the stored attributes for a device with a fully-constructed
    /// `DeviceAttributes` value.
    ///
    /// Unlike [`update_attributes`], this does **not** trigger platform
    /// auto-detection or stale-override cleanup.  It is intended for API
    /// handlers that have already computed the final attribute state and just
    /// need to persist it.
    pub async fn update_attributes_raw(
        &self,
        uuid: &Uuid,
        attrs: &common::device_attributes::DeviceAttributes,
    ) -> anyhow::Result<()> {
        self.conn
            .execute(
                "UPDATE devices SET attributes = ?1 WHERE uuid = ?2",
                (serde_json::to_string(attrs)?, *uuid),
            )
            .await?;
        Ok(())
    }

    pub async fn create_plan(&self, plan: &Plan) -> anyhow::Result<i64> {
        crate::plans::store::create_plan(self.conn, plan).await
    }

    #[cfg(test)]
    pub async fn get_active_plan_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> anyhow::Result<Option<Plan>> {
        crate::plans::store::get_active_plan_for_device(self.conn, device_uuid).await
    }

    /// Handle device boot event
    ///
    /// Called when a device boots via iPXE. If the current action's advance_on_boot()
    /// returns true, the action is automatically marked as successful and the plan advances.
    /// This is used for actions like RebootDevice that complete when the device boots.
    pub async fn on_boot(&self, device_uuid: &Uuid) -> anyhow::Result<()> {
        // Get the current active plan
        let mut plan =
            match crate::plans::store::get_active_plan_for_device(self.conn, device_uuid).await? {
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
            crate::plans::store::update_plan_status(
                self.conn,
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
        let mut plan =
            match crate::plans::store::get_active_plan_for_device(self.conn, device_uuid).await? {
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
        crate::plans::store::update_plan_status(
            self.conn,
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
        let mut plan =
            match crate::plans::store::get_active_plan_for_device(self.conn, device_uuid).await? {
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
        crate::plans::store::update_plan_status(
            self.conn,
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

    /// Auto-detect and assign platform to a device based on its hardware attributes
    ///
    /// This method is called after successful hardware discovery to automatically
    /// identify or create a platform that matches the device's configuration.
    pub async fn auto_detect_platform(&self, device_uuid: &Uuid) -> anyhow::Result<()> {
        log::info!("Auto-detecting platform for device {}", device_uuid);

        // Get device attributes
        let device = store::get_device(self.conn, device_uuid).await?;
        let attrs = &device.attributes;

        // Perform platform detection (pass boot_mode for firmware-aware matching)
        let platform_id = crate::platforms::detect_or_create_platform(
            self.conn,
            &attrs.disks,
            &attrs.network_interfaces,
            &attrs.cpus,
            &attrs.memory,
            attrs.boot_mode,
        )
        .await?;

        // Assign platform to device
        store::assign_platform_to_device(self.conn, device_uuid, platform_id).await?;

        log::info!(
            "Assigned platform {} to device {}",
            platform_id,
            device_uuid
        );

        Ok(())
    }

    /// Add a warning message to device warnings
    /// Used to track platform detection status and other device-related alerts
    pub async fn add_device_warning(
        &self,
        device_uuid: &Uuid,
        warning: &str,
    ) -> anyhow::Result<()> {
        // Get current device attributes
        let device = store::get_device(self.conn, device_uuid).await?;

        // Append new warning to existing warnings
        let mut warnings = device.attributes.warnings;
        warnings.push(warning.to_string());

        let mut attrs = serde_json::Map::new();
        attrs.insert(
            "warnings".to_string(),
            serde_json::Value::Array(
                warnings
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        store::update_attributes(self.conn, device_uuid, attrs).await
    }

    pub async fn start_lifecycle_transition(
        &self,
        device_uuid: &Uuid,
        to_state: DeviceLifecycle,
    ) -> anyhow::Result<i64> {
        // Get current device lifecycle
        let current_lifecycle =
            crate::lifecycle::store::get_device_lifecycle(self.conn, device_uuid)
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
        if let Some(_active_transition) =
            crate::lifecycle::store::get_active_transition_for_device(self.conn, device_uuid)
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
        let plan_id = crate::plans::store::create_plan(self.conn, &plan).await?;

        // Create lifecycle transition
        let transition =
            LifecycleTransition::new(*device_uuid, current_lifecycle, to_state, Some(plan_id));

        let transition_id =
            crate::lifecycle::store::create_transition(self.conn, &transition).await?;

        // Get the newly created plan to access its current action
        let plan = crate::plans::store::get_active_plan_for_device(self.conn, device_uuid)
            .await?
            .expect("Plan should exist immediately after creation");

        // If there's a current action, run its start() hook
        if let Some(action) = plan.get_current_action() {
            let device = store::get_device(self.conn, device_uuid).await?;
            let ctx = crate::plans::actions::ActionContext {
                device: &device,
                conn: self.conn,
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
        crate::lifecycle::store::get_device_lifecycle(self.conn, device_uuid).await
    }

    pub async fn get_active_transition_for_device(
        &self,
        device_uuid: &Uuid,
    ) -> anyhow::Result<Option<LifecycleTransition>> {
        crate::lifecycle::store::get_active_transition_for_device(self.conn, device_uuid).await
    }

    pub async fn get_device_transitions(
        &self,
        device_uuid: &Uuid,
        include_completed: bool,
    ) -> anyhow::Result<Vec<LifecycleTransition>> {
        crate::lifecycle::store::get_transitions_for_device(
            self.conn,
            device_uuid,
            include_completed,
        )
        .await
    }

    async fn handle_plan_completion_success(&self, plan_id: i64) -> anyhow::Result<()> {
        // Find the lifecycle transition associated with this plan
        if let Some(transition) =
            crate::lifecycle::store::get_transition_by_plan_id(self.conn, plan_id).await?
        {
            // Update device lifecycle to the target state
            crate::lifecycle::store::update_device_lifecycle(
                self.conn,
                &transition.device_uuid,
                transition.to_state.clone(),
            )
            .await?;

            // Complete the transition successfully
            crate::lifecycle::store::complete_transition(
                self.conn,
                transition.id.unwrap(),
                true,
                None,
            )
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
        if let Some(transition) =
            crate::lifecycle::store::get_transition_by_plan_id(self.conn, plan_id).await?
        {
            // Move device to broken state on failure
            crate::lifecycle::store::update_device_lifecycle(
                self.conn,
                &transition.device_uuid,
                DeviceLifecycle::Broken,
            )
            .await?;

            // Complete the transition with failure
            crate::lifecycle::store::complete_transition(
                self.conn,
                transition.id.unwrap(),
                false,
                Some(error_message),
            )
            .await?;
        }

        Ok(())
    }

    pub async fn get_device(&self, uuid: &Uuid) -> anyhow::Result<Device> {
        store::get_device(self.conn, uuid).await
    }

    pub async fn get_all_devices(&self) -> anyhow::Result<Vec<Device>> {
        store::get_all_devices(self.conn).await
    }

    pub async fn find_device_by_mac(&self, mac: &str) -> anyhow::Result<Option<Uuid>> {
        store::find_device_by_mac(self.conn, mac).await
    }

    pub async fn set_device_mac_address(&self, uuid: &Uuid, mac: &str) -> anyhow::Result<()> {
        store::set_mac_address(self.conn, uuid, mac).await
    }

    pub async fn set_device_ip_address(
        &self,
        uuid: &Uuid,
        ip: &str,
        mac: &str,
    ) -> anyhow::Result<()> {
        store::set_ip_address(self.conn, uuid, ip, mac).await
    }

    pub async fn get_network_interfaces(
        &self,
        uuid: &Uuid,
    ) -> anyhow::Result<Vec<NetworkInterface>> {
        store::get_network_interfaces(self.conn, uuid).await
    }

    pub async fn set_network_interfaces(
        &self,
        uuid: &Uuid,
        interfaces: &[NetworkInterface],
    ) -> anyhow::Result<()> {
        store::set_network_interfaces(self.conn, uuid, interfaces).await
    }

    // Platform assignment methods

    pub async fn assign_platform_to_device(
        &self,
        device_uuid: &Uuid,
        platform_id: i64,
    ) -> anyhow::Result<()> {
        store::assign_platform_to_device(self.conn, device_uuid, platform_id).await
    }

    pub async fn get_device_platform_id(&self, device_uuid: &Uuid) -> anyhow::Result<Option<i64>> {
        store::get_device_platform_id(self.conn, device_uuid).await
    }

    #[cfg(test)]
    pub async fn list_devices_with_platform(&self, platform_id: i64) -> anyhow::Result<Vec<Uuid>> {
        store::list_devices_with_platform(self.conn, platform_id).await
    }

    // Role assignment methods

    /// Assign a role to a device, validating disk layout labels and firmware compatibility.
    ///
    /// Validation checks:
    /// 1. If the role's disk layout uses platform labels, the device must have a platform
    ///    assigned and all labels must exist in that platform.
    /// 2. If the role has a `firmware_mode` constraint and the device has a detected
    ///    `boot_mode`, they must match. Devices without a detected `boot_mode` are allowed
    ///    through with a warning (device-scan may not have run yet).
    pub async fn assign_role_to_device(
        &self,
        device_uuid: &Uuid,
        role_id: i64,
    ) -> anyhow::Result<()> {
        // Get the role to check its disk layout and firmware constraint
        let role = roles::store::get(self.conn, role_id).await?;

        // Get device (needed for label validation and firmware check)
        let device = store::get_device(self.conn, device_uuid).await?;

        // Check if disk layout uses labels
        if crate::disk_layout::layout_uses_labels(&role.disk_layout) {
            let platform_id = device.platform_id.ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot assign role '{}': disk layout uses platform labels but device has no platform assigned",
                    role.name
                )
            })?;

            // Validate labels exist in platform
            let platform = platforms::store::get(self.conn, platform_id).await?;
            crate::disk_layout::validate_layout_against_platform(
                &role.disk_layout,
                &platform.attributes,
            )?;
        }

        // Validate firmware mode compatibility
        if let Some(required_firmware) = role.firmware_mode {
            match device.attributes.boot_mode {
                Some(actual_mode) if actual_mode != required_firmware => {
                    return Err(anyhow::anyhow!(
                        "Firmware mismatch: role '{}' requires {} but device has {}",
                        role.name,
                        required_firmware,
                        actual_mode
                    ));
                }
                None => {
                    // Device scan not yet run — warn but allow assignment
                    log::warn!(
                        "Assigning role '{}' (firmware_mode={}) to device {} with no detected boot_mode (device-scan may not have run yet)",
                        role.name,
                        required_firmware,
                        device_uuid
                    );
                }
                Some(_) => {} // Modes match — proceed
            }
        }

        store::assign_role_to_device(self.conn, device_uuid, role_id).await
    }

    pub async fn get_device_role_id(&self, device_uuid: &Uuid) -> anyhow::Result<Option<i64>> {
        store::get_device_role_id(self.conn, device_uuid).await
    }

    pub async fn create_pending_device(
        &self,
        mac_address: &str,
        network_id: i64,
    ) -> anyhow::Result<i64> {
        store::create_pending_device(self.conn, mac_address, network_id).await
    }

    pub async fn find_pending_device_by_mac(
        &self,
        mac_address: &str,
    ) -> anyhow::Result<Option<i64>> {
        store::find_pending_device_by_mac(self.conn, mac_address).await
    }

    pub async fn complete_pending_device(
        &self,
        mac_address: &str,
        device_uuid: &Uuid,
    ) -> anyhow::Result<()> {
        store::complete_pending_device(self.conn, mac_address, device_uuid).await
    }

    pub async fn get_pending_devices(&self) -> anyhow::Result<Vec<PendingDevice>> {
        store::get_pending_devices(self.conn).await
    }

    pub async fn delete_pending_device(&self, id: i64) -> anyhow::Result<()> {
        store::delete_pending_device(self.conn, id).await
    }

    pub async fn delete_device(&self, uuid: &Uuid) -> anyhow::Result<()> {
        // CRITICAL: Clean up reservations BEFORE deleting device
        // (interfaces stored in device JSON, lost after deletion)

        // 1. Delete for all discovered interfaces
        let interfaces = store::get_network_interfaces(self.conn, uuid)
            .await
            .unwrap_or_default();
        for nic in &interfaces {
            match crate::dhcp::store::delete_static_reservations_by_mac(self.conn, &nic.mac_address)
                .await
            {
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
        if let Ok(device) = store::get_device(self.conn, uuid).await
            && let Some(mac) = &device.attributes.mac_address
            && !interfaces.iter().any(|nic| &nic.mac_address == mac)
        {
            let _ = crate::dhcp::store::delete_static_reservations_by_mac(self.conn, mac).await;
        }

        // 3. Delete device (cascades to plans, transitions)
        store::delete_device(self.conn, uuid).await
    }

    pub async fn find_device_by_bmc_mac(&self, mac: &str) -> anyhow::Result<Option<Uuid>> {
        store::find_device_by_bmc_mac(self.conn, mac).await
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
        let device = store::get_device(self.conn, uuid).await?;
        Ok(device.attributes.bmc.and_then(|bmc| bmc.ip_address))
    }

    /// Get BMC credentials from device attributes
    async fn get_bmc_credentials(&self, uuid: &Uuid) -> anyhow::Result<Option<(String, String)>> {
        let device = store::get_device(self.conn, uuid).await?;
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
    use crate::{
        database, database::DatabaseConnectionFactory, plans::PlanStatus, test_connection_factory,
    };

    /// Sets up a test database and returns the connection.
    ///
    /// Each test constructs `Director::new(&conn)` locally so that the Director
    /// borrows the connection for the duration of the test.
    async fn setup_test_db(factory: DatabaseConnectionFactory) -> database::Connection {
        database::run_migrations(&factory).await.unwrap()
    }

    #[tokio::test]
    async fn test_single_active_plan_constraint() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440006").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create first plan — InstallOs is valid as a non-first action
        let first_actions = vec![
            crate::plans::Action::DiscoverHardware,
            crate::plans::Action::InstallOs,
        ];
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
            crate::plans::Action::DiscoverHardware
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
        assert_eq!(plan.actions[0], crate::plans::Action::DiscoverHardware);
        assert_eq!(plan.status, PlanStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_all_devices() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);

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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let boot_target = director.next_boot_target(&test_uuid, 600).await.unwrap();
        match boot_target {
            BootTarget::AgentImage { action, cmdline: _ } => {
                assert_eq!(action, "daemon");
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
        let boot_target = director.next_boot_target(&test_uuid, 600).await.unwrap();
        assert!(
            matches!(boot_target, BootTarget::AgentImage { action, cmdline: _ } if action == "daemon")
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

        // After discovery, device is Unprovisioned (no OS installed yet) and has no active plan.
        // It should sleep and retry rather than attempt to boot local disk.
        let boot_target = director.next_boot_target(&test_uuid, 600).await.unwrap();
        assert!(
            matches!(boot_target, BootTarget::SleepReboot { seconds: 600 }),
            "Expected SleepReboot after discovery completion for Unprovisioned device, got {boot_target:?}"
        );
    }

    #[tokio::test]
    async fn test_reboot_with_bmc_info() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440023").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a realistic plan: DiscoverHardware first, then RebootDevice, then InstallOs.
        // RebootDevice cannot be the first action (daemon may already be running).
        let actions = vec![
            crate::plans::Action::DiscoverHardware,
            crate::plans::Action::RebootDevice,
            crate::plans::Action::InstallOs,
        ];
        let plan = crate::plans::Plan::new(test_uuid, actions);
        let plan_id = director.create_plan(&plan).await.unwrap();

        // Simulate DiscoverHardware completing by advancing current_step to 1
        // so that RebootDevice is now the current action.
        conn.execute(
            "UPDATE plans SET status = 'running', current_step = 1 WHERE id = ?1",
            (plan_id,),
        )
        .await
        .unwrap();

        // Verify RebootDevice is now the current action
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 1);
        assert_eq!(plan.actions[1], crate::plans::Action::RebootDevice);

        // Call on_boot - should advance past RebootDevice
        director.on_boot(&test_uuid).await.unwrap();

        // Verify plan advanced to InstallOs
        let active_plan = director
            .get_active_plan_for_device(&test_uuid)
            .await
            .unwrap();
        assert!(active_plan.is_some());
        let plan = active_plan.unwrap();
        assert_eq!(plan.current_step, 2);
        assert_eq!(plan.actions[2], crate::plans::Action::InstallOs);
    }

    #[tokio::test]
    async fn test_on_boot_does_not_advance_other_actions() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
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
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440033").unwrap();

        // Create a DHCP network using the shared connection
        let network = crate::dhcp::store::create_network(
            &conn,
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
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];
        director
            .set_network_interfaces(&test_uuid, &interfaces)
            .await
            .unwrap();

        // Create static reservation
        crate::dhcp::store::create_or_update_static_reservation(
            &conn,
            network.id,
            mac,
            "10.0.0.100",
            None,
        )
        .await
        .unwrap();

        // Verify reservation exists
        let reservation = crate::dhcp::store::get_static_reservation(&conn, network.id, mac)
            .await
            .unwrap();
        assert!(reservation.is_some());

        // Delete device
        director.delete_device(&test_uuid).await.unwrap();

        // Verify reservation was deleted
        let reservation = crate::dhcp::store::get_static_reservation(&conn, network.id, mac)
            .await
            .unwrap();
        assert!(reservation.is_none());
    }

    #[tokio::test]
    async fn test_delete_device_removes_reservations_for_multiple_nics() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440034").unwrap();

        // Create DHCP network using the shared connection
        let network = crate::dhcp::store::create_network(
            &conn,
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
                speed_mbps: Some(10000),
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: mac2.to_string(),
                ip_address: Some("10.0.0.102".to_string()),
                network_id: Some(network.id),
                speed_mbps: Some(10000),
                disabled: false,
                warning_label: None,
            },
        ];
        director
            .set_network_interfaces(&test_uuid, &interfaces)
            .await
            .unwrap();

        // Create static reservations for both MACs
        crate::dhcp::store::create_or_update_static_reservation(
            &conn,
            network.id,
            mac1,
            "10.0.0.101",
            None,
        )
        .await
        .unwrap();
        crate::dhcp::store::create_or_update_static_reservation(
            &conn,
            network.id,
            mac2,
            "10.0.0.102",
            None,
        )
        .await
        .unwrap();

        // Verify both reservations exist
        assert!(
            crate::dhcp::store::get_static_reservation(&conn, network.id, mac1)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            crate::dhcp::store::get_static_reservation(&conn, network.id, mac2)
                .await
                .unwrap()
                .is_some()
        );

        // Delete device
        director.delete_device(&test_uuid).await.unwrap();

        // Verify both reservations were deleted
        assert!(
            crate::dhcp::store::get_static_reservation(&conn, network.id, mac1)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            crate::dhcp::store::get_static_reservation(&conn, network.id, mac2)
                .await
                .unwrap()
                .is_none()
        );
    }

    // Platform detection status tests

    #[tokio::test]
    async fn test_platform_detection_status_success() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440050").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a platform first
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![crate::platforms::PlatformDisk {
                size_gb: 480,
                disk_type: crate::platforms::DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![crate::platforms::PlatformNic {
                logical: "eth0".to_string(),
                speed_mbps: Some(10000),
                label: Some("NIC1".to_string()),
            }],
            cpus: vec![crate::platforms::PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        };
        crate::platforms::store::create(&conn, "Test Platform", None, &platform_attrs, None)
            .await
            .unwrap();

        // Update device with matching hardware info
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "disks".to_string(),
            serde_json::json!([{
                "name": "sda",
                "size": 480,
                "disk_type": "ssd",
                "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
            }]),
        );
        hardware_attrs.insert(
            "network_interfaces".to_string(),
            serde_json::json!([{
                "interface_name": "eth0",
                "mac_address": "aa:bb:cc:dd:ee:ff",
                "speed_mbps": 10000
            }]),
        );
        hardware_attrs.insert(
            "cpus".to_string(),
            serde_json::json!([{
                "designation": "E3-1240 v3",
                "manufacturer": "Intel Corporation",
                "cores": 4
            }]),
        );
        hardware_attrs.insert(
            "memory".to_string(),
            serde_json::json!([{
                "size_mb": 32768
            }]),
        );

        director
            .update_attributes(&test_uuid, hardware_attrs)
            .await
            .unwrap();

        // Verify platform was assigned (no warning on success)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(device.platform_id.is_some());
        assert!(device.attributes.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_platform_detection_status_failed() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440051").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Update device with hardware info that won't match any platform
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "disks".to_string(),
            serde_json::json!([{
                "name": "nvme0n1",
                "size": 1000,
                "disk_type": "nvme",
                "path": "/dev/nvme0n1"
            }]),
        );
        hardware_attrs.insert(
            "network_interfaces".to_string(),
            serde_json::json!([{
                "interface_name": "ens0",
                "mac_address": "aa:bb:cc:dd:ee:ff",
                "speed_mbps": 10000
            }]),
        );
        hardware_attrs.insert(
            "cpus".to_string(),
            serde_json::json!([{
                "designation": "Xeon E5-2680 v4",
                "manufacturer": "Intel Corporation",
                "cores": 14
            }]),
        );
        hardware_attrs.insert(
            "memory".to_string(),
            serde_json::json!([{
                "size_mb": 32768
            }]),
        );

        director
            .update_attributes(&test_uuid, hardware_attrs)
            .await
            .unwrap();

        // Verify platform was auto-created (no warning on success)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(device.platform_id.is_some());
        assert!(device.attributes.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_platform_detection_status_manual() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440052").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a platform
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![],
            memory_gib: 0,
        };
        let platform =
            crate::platforms::store::create(&conn, "Manual Platform", None, &platform_attrs, None)
                .await
                .unwrap();

        // Manually assign platform
        director
            .assign_platform_to_device(&test_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Verify platform was manually assigned (no warning on manual assignment)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert_eq!(device.platform_id, Some(platform.id.unwrap()));
    }

    #[tokio::test]
    async fn test_platform_detection_status_no_hardware_info() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440053").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Update device with non-hardware attributes
        let mut attrs = serde_json::Map::new();
        attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("test-host".to_string()),
        );

        director.update_attributes(&test_uuid, attrs).await.unwrap();

        // Verify no warnings (no hardware info = no detection attempt)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(device.attributes.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_platform_detection_creates_new_platform_on_no_match() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440056").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Verify no platforms exist initially
        let platforms = crate::platforms::store::list(&conn).await.unwrap();
        assert_eq!(
            platforms.len(),
            0,
            "Should start with empty platform database"
        );

        // Update device with hardware info that won't match any existing platform
        // (since database is empty, this will auto-create a new platform)
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "disks".to_string(),
            serde_json::json!([{
                "name": "nvme0n1",
                "size": 960,
                "disk_type": "nvme",
                "path": "/dev/nvme0n1"
            }]),
        );
        hardware_attrs.insert(
            "network_interfaces".to_string(),
            serde_json::json!([{
                "interface_name": "ens0",
                "mac_address": "aa:bb:cc:dd:ee:56",
                "speed_mbps": 25000
            }]),
        );
        hardware_attrs.insert(
            "cpus".to_string(),
            serde_json::json!([{
                "designation": "Xeon Gold 6248R",
                "manufacturer": "Intel Corporation",
                "model": "Intel(R) Xeon(R) Gold 6248R @ 2.2 GHz",
                "cores": 24
            }]),
        );
        hardware_attrs.insert(
            "memory".to_string(),
            serde_json::json!([
                {"size_mb": 32768},  // 32 GB per module
                {"size_mb": 32768},
                {"size_mb": 32768},
                {"size_mb": 32768}
            ]),
        );

        director
            .update_attributes(&test_uuid, hardware_attrs)
            .await
            .unwrap();

        // Verify a new platform was auto-created
        let platforms = crate::platforms::store::list(&conn).await.unwrap();
        assert_eq!(
            platforms.len(),
            1,
            "Platform should be auto-created when no match exists"
        );

        // Verify device was assigned the new platform (no warning on success)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(
            device.platform_id.is_some(),
            "Device should have platform assigned"
        );
        assert!(
            device.attributes.warnings.is_empty(),
            "No warning on successful detection"
        );

        // Verify the created platform has the correct hardware attributes
        let platform = crate::platforms::store::get(&conn, device.platform_id.unwrap())
            .await
            .unwrap();
        assert_eq!(platform.attributes.disks.len(), 1);
        assert_eq!(platform.attributes.nics.len(), 1);
        assert_eq!(platform.attributes.cpus.len(), 1);
        assert_eq!(platform.attributes.memory_gib, 128); // 4 modules × 32 GB
    }

    #[tokio::test]
    async fn test_platform_detection_status_partial_hardware_info() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440054").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Update with only disks (missing other fields for complete match)
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "disks".to_string(),
            serde_json::json!([{
                "name": "sda",
                "size": 480,
                "disk_type": "ssd"
            }]),
        );

        director
            .update_attributes(&test_uuid, hardware_attrs)
            .await
            .unwrap();

        // Verify platform was auto-created (no warning on success)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(
            device.platform_id.is_some(),
            "Platform should be auto-created"
        );
        assert!(
            device.attributes.warnings.is_empty(),
            "No warning on successful detection"
        );
    }

    #[tokio::test]
    async fn test_platform_detection_status_manual_overrides_auto() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440055").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // First, auto-detect (will create a platform)
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "disks".to_string(),
            serde_json::json!([{
                "name": "sda",
                "size": 480,
                "disk_type": "ssd",
                "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
            }]),
        );
        hardware_attrs.insert("network_interfaces".to_string(), serde_json::json!([]));
        hardware_attrs.insert("cpus".to_string(), serde_json::json!([]));
        hardware_attrs.insert("memory".to_string(), serde_json::json!([]));

        director
            .update_attributes(&test_uuid, hardware_attrs)
            .await
            .unwrap();

        // Verify auto-detection succeeded (no warning on success)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert!(device.attributes.warnings.is_empty());
        let auto_platform_id = device.platform_id.unwrap();

        // Create a different platform and manually assign it
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![],
            memory_gib: 0,
        };
        let manual_platform =
            crate::platforms::store::create(&conn, "Manual Override", None, &platform_attrs, None)
                .await
                .unwrap();

        director
            .assign_platform_to_device(&test_uuid, manual_platform.id.unwrap())
            .await
            .unwrap();

        // Verify platform changed (no warning on manual assignment)
        let device = director.get_device(&test_uuid).await.unwrap();
        assert_eq!(device.platform_id, Some(manual_platform.id.unwrap()));
        assert_ne!(device.platform_id, Some(auto_platform_id));
        assert!(device.attributes.warnings.is_empty());
    }

    // ========== Role Assignment Validation Tests ==========

    #[tokio::test]
    async fn test_assign_role_with_labels_no_platform_fails() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440060").unwrap();

        // Register device (no platform)
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create role with label-based layout
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "ROOT".to_string(), // Platform label
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "label-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Assign role should fail - labels used but no platform
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no platform assigned")
        );
    }

    #[tokio::test]
    async fn test_assign_role_with_missing_labels_fails() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440061").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create platform without the label the role needs
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![crate::platforms::PlatformDisk {
                size_gb: 480,
                disk_type: crate::platforms::DiskType::Ssd,
                label: Some("DATA1".to_string()), // Has DATA1, not ROOT
            }],
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };
        let platform =
            crate::platforms::store::create(&conn, "Test Platform", None, &platform_attrs, None)
                .await
                .unwrap();
        director
            .assign_platform_to_device(&test_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Create role that references ROOT label (not in platform)
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "missing-label-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Assign should fail - label not in platform
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ROOT"));
    }

    #[tokio::test]
    async fn test_assign_role_with_matching_labels_succeeds() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440062").unwrap();

        // Register device
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create platform with ROOT label
        let platform_attrs = crate::platforms::PlatformAttributes {
            disks: vec![crate::platforms::PlatformDisk {
                size_gb: 480,
                disk_type: crate::platforms::DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };
        let platform =
            crate::platforms::store::create(&conn, "Test Platform", None, &platform_attrs, None)
                .await
                .unwrap();
        director
            .assign_platform_to_device(&test_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Create role that uses ROOT label
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "label-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Assign should succeed - label matches platform
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assign_role_with_paths_no_platform_succeeds() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440063").unwrap();

        // Register device (no platform)
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create role with path-based layout (no labels)
        let layout = common::disk_layout::DiskLayout {
            disks: vec![common::disk_layout::DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(), // Absolute path, not a label
                partition_table: "gpt".to_string(),
                partitions: vec![],
            }],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "path-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Assign should succeed - no labels, no platform needed
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assign_role_firmware_mismatch_fails() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440066").unwrap();

        // Register device and set its boot_mode to BIOS via attribute update
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        let mut attrs = serde_json::Map::new();
        attrs.insert("boot_mode".to_string(), serde_json::json!("bios"));
        // Update only attributes (not hardware discovery fields, to avoid platform detection)
        crate::director::store::update_attributes(&conn, &test_uuid, attrs)
            .await
            .unwrap();

        // Create a UEFI-only role
        let layout = common::disk_layout::DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "uefi-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        // Assign should fail — device is BIOS, role requires UEFI
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Firmware mismatch"),
            "Expected 'Firmware mismatch' in error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_assign_role_firmware_match_succeeds() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440067").unwrap();

        // Register device and set its boot_mode to UEFI
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        let mut attrs = serde_json::Map::new();
        attrs.insert("boot_mode".to_string(), serde_json::json!("uefi"));
        crate::director::store::update_attributes(&conn, &test_uuid, attrs)
            .await
            .unwrap();

        // Create a UEFI-only role
        let layout = common::disk_layout::DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "uefi-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        // Assign should succeed — both are UEFI
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assign_role_firmware_constrained_role_no_device_boot_mode_warns_and_proceeds() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440068").unwrap();

        // Register device (no boot_mode set — device-scan not yet run)
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create a UEFI-constrained role
        let layout = common::disk_layout::DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };
        let role = crate::roles::store::create(
            &conn,
            "uefi-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &layout,
            None,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        // Assign should succeed (with a warning) — device has no boot_mode yet
        let result = director
            .assign_role_to_device(&test_uuid, role.id.unwrap())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unknown_device_sleep_reboot() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        // UUID that was never registered
        let unknown_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440064").unwrap();

        let boot_target = director.next_boot_target(&unknown_uuid, 600).await.unwrap();
        assert!(
            matches!(boot_target, BootTarget::SleepReboot { seconds: 600 }),
            "Expected SleepReboot for unknown device, got {boot_target:?}"
        );
    }

    #[tokio::test]
    async fn test_provisioned_device_boots_local_disk() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440065").unwrap();

        // Register device and directly set lifecycle to Provisioned via the store.
        // This avoids running through the full plan machinery and keeps the test focused
        // on the boot-target logic.
        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        crate::lifecycle::store::update_device_lifecycle(
            &conn,
            &test_uuid,
            DeviceLifecycle::Provisioned,
        )
        .await
        .unwrap();

        // Verify lifecycle is Provisioned
        let lifecycle = director.get_device_lifecycle(&test_uuid).await.unwrap();
        assert_eq!(lifecycle, Some(DeviceLifecycle::Provisioned));

        // A provisioned device with no active plan should boot local disk
        let boot_target = director.next_boot_target(&test_uuid, 600).await.unwrap();
        assert!(
            matches!(boot_target, BootTarget::LocalDisk),
            "Expected LocalDisk for Provisioned device, got {boot_target:?}"
        );
    }
}
