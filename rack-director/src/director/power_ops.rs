//! Power-management methods on `Director`.
//!
//! This file contains the `impl Director` block for all OOB power operations:
//! resolving a driver, querying state, executing user-requested actions, and
//! the pre-plan "power kick" that ensures the device is running the agent before
//! a lifecycle transition begins.
//!
//! Split from `director/mod.rs` to keep core device-management logic separate
//! from power-specific code.  Callers inside this crate continue to call these
//! methods on `Director<'a>` unchanged — Rust allows `impl` blocks to be spread
//! across files within the same module.

use uuid::Uuid;

use super::power::ops::{PowerAction, action_requires_boot, apply_power_kick};
use super::power::{self, PowerState};
use super::store;
use crate::plans::actions::Action;

impl<'a> super::Director<'a> {
    /// Ensure the device is powered and will boot for the given plan action.
    ///
    /// Called immediately before `action.start()` in `start_lifecycle_transition`.
    /// Short-circuits without any BMC call when:
    /// 1. The action does not require the device to boot (e.g. `RebootDevice`).
    /// 2. The agent is already polling in daemon mode (heartbeat within `DAEMON_HEARTBEAT_WINDOW`).
    /// 3. The device has no BMC configured.
    ///
    /// All power operations are **best-effort**: errors are logged but never propagated.
    /// Always returns `Ok(())`.
    pub async fn ensure_powered_for_plan(
        &self,
        device: &store::Device,
        action: &Action,
    ) -> anyhow::Result<()> {
        if !action_requires_boot(action) {
            return Ok(());
        }

        if power::is_in_daemon_mode(
            device.last_polled_at.as_deref(),
            power::DAEMON_HEARTBEAT_WINDOW,
        ) {
            log::info!(
                "Device {} is in daemon mode (last_polled_at={}), skipping power kick",
                device.uuid,
                device.last_polled_at.as_deref().unwrap_or("none")
            );
            return Ok(());
        }

        let driver = match self.power_driver_for(&device.uuid).await? {
            Some(d) => d,
            None => {
                log::info!(
                    "No BMC driver available for device {}, skipping power kick",
                    device.uuid
                );
                return Ok(());
            }
        };

        if let Err(e) = apply_power_kick(driver.as_ref()).await {
            log::warn!(
                "apply_power_kick returned error for device {}: {}",
                device.uuid,
                e
            );
        }

        Ok(())
    }

    /// Issue a power reset to the device's BMC using the best available driver.
    ///
    /// Probes the BMC for Redfish support and falls back to IPMI.  This is a
    /// best-effort operation: if the BMC IP or credentials are missing, or if
    /// the power command fails, the error is logged but not propagated.  This
    /// allows lifecycle transitions to continue even when OOB power is unavailable.
    pub async fn reboot(&self, uuid: &Uuid) -> anyhow::Result<()> {
        match self.power_driver_for(uuid).await? {
            Some(driver) => {
                if let Err(e) = driver.power_reset().await {
                    log::warn!(
                        "Power reset failed for device {} via {} driver: {}",
                        uuid,
                        driver.kind(),
                        e
                    );
                }
            }
            None => {
                log::info!(
                    "No BMC configured for device {}, skipping power reset",
                    uuid
                );
            }
        }
        Ok(())
    }

    /// Resolve the best available power driver for a device.
    ///
    /// Looks up the device's BMC IP and credentials, then calls
    /// [`power::resolve_power_driver`] to probe for Redfish support and fall
    /// back to IPMI if unavailable.
    ///
    /// Returns `Ok(None)` when the device has no BMC IP or no credentials
    /// stored — callers should log and skip the power operation in that case.
    ///
    /// This is the foundation for future power endpoints and the kick logic.
    /// Callers that need power operations should call this method and act on
    /// the returned driver.
    pub async fn power_driver_for(
        &self,
        uuid: &Uuid,
    ) -> anyhow::Result<Option<Box<dyn power::PowerDriver>>> {
        let ip = match self.get_bmc_ip(uuid).await? {
            Some(ip) => ip,
            None => return Ok(None),
        };
        let (username, password) = match self.get_bmc_credentials(uuid).await? {
            Some(creds) => creds,
            None => return Ok(None),
        };
        Ok(Some(
            power::resolve_power_driver(&ip, &username, &password, self.power_config).await,
        ))
    }

    /// Query the current power state and driver kind for a device.
    ///
    /// Never errors — a missing BMC, unreachable BMC, or driver failure all
    /// degrade to `(PowerState::Unknown, None)` so that the UI power badge
    /// can always render without blocking the page.
    pub async fn power_status(&self, uuid: &Uuid) -> (PowerState, Option<String>) {
        match self.power_driver_for(uuid).await {
            Ok(Some(d)) => {
                let state = d.power_state().await.unwrap_or(PowerState::Unknown);
                (state, Some(d.kind().to_string()))
            }
            _ => (PowerState::Unknown, None),
        }
    }

    /// Execute a UI-requested power action on a device.
    ///
    /// `PowerAction::Off` issues a **hard** (immediate) power-off — not a
    /// graceful OS shutdown — because hosts often run the rack-agent in an
    /// initramfs that cannot honor ACPI soft-off, and this matches the UI
    /// confirm dialog's promise of immediate power loss.
    ///
    /// Returns:
    /// - `Ok(true)`  – action was issued successfully.
    /// - `Ok(false)` – device has no BMC configured (caller should return 404).
    /// - `Err(_)`    – BMC driver reported a failure (caller should return 502).
    pub async fn power_action(&self, uuid: &Uuid, action: PowerAction) -> anyhow::Result<bool> {
        let driver = match self.power_driver_for(uuid).await? {
            Some(d) => d,
            None => return Ok(false),
        };

        match action {
            PowerAction::On => driver.power_on().await?,
            PowerAction::Off => driver.power_off(false).await?,
            PowerAction::Cycle => driver.power_cycle().await?,
        }

        Ok(true)
    }

    /// Get BMC IP address from device attributes
    pub(super) async fn get_bmc_ip(&self, uuid: &Uuid) -> anyhow::Result<Option<String>> {
        let device = store::get_device(self.conn, uuid).await?;
        Ok(device.attributes.bmc.and_then(|bmc| bmc.ip_address))
    }

    /// Get BMC credentials from device attributes
    pub(super) async fn get_bmc_credentials(
        &self,
        uuid: &Uuid,
    ) -> anyhow::Result<Option<(String, String)>> {
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
    use uuid::Uuid;

    use crate::{
        database,
        database::DatabaseConnectionFactory,
        director::{Architecture, Director, store},
        plans::actions::Action,
        test_connection_factory,
    };

    async fn setup_test_db(factory: DatabaseConnectionFactory) -> database::Connection {
        database::run_migrations(&factory).await.unwrap()
    }

    // ========== ensure_powered_for_plan daemon-mode skip test ==========

    #[tokio::test]
    async fn test_ensure_powered_for_plan_skips_when_in_daemon_mode() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440070").unwrap();

        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Manually set last_polled_at to now (within the 15s window) so
        // is_in_daemon_mode returns true and the kick is skipped.
        conn.execute(
            "UPDATE devices SET last_polled_at = CURRENT_TIMESTAMP WHERE uuid = ?1",
            (test_uuid,),
        )
        .await
        .unwrap();

        let device = store::get_device(&conn, &test_uuid).await.unwrap();

        // DiscoverHardware requires boot, but daemon mode is active → should skip
        let result = director
            .ensure_powered_for_plan(&device, &Action::DiscoverHardware)
            .await;
        assert!(result.is_ok());
        // No BMC configured either, but we never reach that check due to daemon mode
    }

    #[tokio::test]
    async fn test_ensure_powered_for_plan_skips_reboot_device() {
        let conn = setup_test_db(test_connection_factory!()).await;
        let director = Director::new(&conn);
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440071").unwrap();

        director
            .register_device(&test_uuid, Architecture::X86_64)
            .await
            .unwrap();

        let device = store::get_device(&conn, &test_uuid).await.unwrap();

        // RebootDevice does not require boot → ensure_powered_for_plan returns immediately
        let result = director
            .ensure_powered_for_plan(&device, &Action::RebootDevice)
            .await;
        assert!(result.is_ok());
    }

    // ========== reboot / BMC helper tests ==========

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
}
