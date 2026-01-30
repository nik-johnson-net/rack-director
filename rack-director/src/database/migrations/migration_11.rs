//! Migration 11: Convert UUID columns from TEXT to BLOB (16 bytes binary)
//!
//! This module handles the data migration for converting TEXT UUIDs to binary BLOBs.
//! The table schemas are created by 11.sql, and this code populates them.

use anyhow::Result;
use rusqlite::{Connection, params};
use uuid::Uuid;

/// Post-migration hook for migration 11: Convert TEXT UUIDs to BLOB
/// Reads TEXT UUIDs from old tables and inserts as BLOB into new tables
pub fn convert_uuids(conn: &Connection) -> Result<()> {
    log::info!("Converting TEXT UUIDs to BLOB format...");

    // Convert each table
    convert_devices_table(conn)?;
    convert_plans_table(conn)?;
    convert_lifecycle_transitions_table(conn)?;
    convert_dhcp_leases_table(conn)?;
    convert_pending_devices_table(conn)?;

    // Replace old tables with new tables
    finalize_migration(conn)?;

    log::info!("UUID conversion complete");
    Ok(())
}

fn convert_devices_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting devices table...");

    let mut stmt = conn.prepare(
        "SELECT id, uuid, created_at, first_seen_at, last_seen_at, attributes, lifecycle, role_id, architecture FROM devices"
    )?;

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let uuid_str: String = row.get(1)?;
        let created_at: Option<String> = row.get(2).ok();
        let first_seen_at: Option<String> = row.get(3).ok();
        let last_seen_at: Option<String> = row.get(4).ok();
        let attributes: String = row.get(5)?;
        let lifecycle: String = row.get(6)?;
        let role_id: Option<i64> = row.get(7)?;
        let architecture: String = row.get(8)?;

        let uuid = Uuid::parse_str(&uuid_str).map_err(|e| {
            anyhow::anyhow!("Failed to parse UUID '{}' in devices: {}", uuid_str, e)
        })?;

        conn.execute(
            "INSERT INTO devices_new (id, uuid, created_at, first_seen_at, last_seen_at, attributes, lifecycle, role_id, architecture)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, uuid, created_at, first_seen_at, last_seen_at, attributes, lifecycle, role_id, architecture],
        )?;
    }

    Ok(())
}

fn convert_plans_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting plans table...");

    let mut stmt = conn.prepare(
        "SELECT id, device_uuid, status, current_step, total_steps, actions, error_message, created_at, started_at, completed_at FROM plans"
    )?;

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let uuid_str: String = row.get(1)?;
        let status: String = row.get(2)?;
        let current_step: i32 = row.get(3)?;
        let total_steps: i32 = row.get(4)?;
        let actions: String = row.get(5)?;
        let error_message: Option<String> = row.get(6)?;
        let created_at: String = row.get(7)?;
        let started_at: Option<String> = row.get(8)?;
        let completed_at: Option<String> = row.get(9)?;

        let uuid = Uuid::parse_str(&uuid_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse UUID '{}' in plans: {}", uuid_str, e))?;

        conn.execute(
            "INSERT INTO plans_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                uuid,
                status,
                current_step,
                total_steps,
                actions,
                error_message,
                created_at,
                started_at,
                completed_at
            ],
        )?;
    }

    Ok(())
}

fn convert_lifecycle_transitions_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting lifecycle_transitions table...");

    let mut stmt = conn.prepare(
        "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message FROM lifecycle_transitions"
    )?;

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let uuid_str: String = row.get(1)?;
        let from_state: String = row.get(2)?;
        let to_state: String = row.get(3)?;
        let plan_id: Option<i64> = row.get(4)?;
        let created_at: String = row.get(5)?;
        let completed_at: Option<String> = row.get(6)?;
        let success: Option<bool> = row.get(7)?;
        let error_message: Option<String> = row.get(8)?;

        let uuid = Uuid::parse_str(&uuid_str).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse UUID '{}' in lifecycle_transitions: {}",
                uuid_str,
                e
            )
        })?;

        conn.execute(
            "INSERT INTO lifecycle_transitions_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                uuid,
                from_state,
                to_state,
                plan_id,
                created_at,
                completed_at,
                success,
                error_message
            ],
        )?;
    }

    Ok(())
}

fn convert_dhcp_leases_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting dhcp_leases table...");

    let mut stmt = conn.prepare(
        "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, created_at, updated_at, network_id FROM dhcp_leases"
    )?;

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let mac_address: String = row.get(1)?;
        let ip_address: String = row.get(2)?;
        let uuid_str: Option<String> = row.get(3)?;
        let lease_start: String = row.get(4)?;
        let lease_end: String = row.get(5)?;
        let state: String = row.get(6)?;
        let hostname: Option<String> = row.get(7)?;
        let created_at: String = row.get(8)?;
        let updated_at: String = row.get(9)?;
        let network_id: Option<i64> = row.get(10)?;

        let device_uuid = uuid_str
            .map(|s| Uuid::parse_str(&s))
            .transpose()
            .map_err(|e| anyhow::anyhow!("Failed to parse UUID in dhcp_leases: {}", e))?;

        conn.execute(
            "INSERT INTO dhcp_leases_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                mac_address,
                ip_address,
                device_uuid,
                lease_start,
                lease_end,
                state,
                hostname,
                created_at,
                updated_at,
                network_id
            ],
        )?;
    }

    Ok(())
}

fn convert_pending_devices_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting pending_devices table...");

    let mut stmt = conn.prepare(
        "SELECT id, mac_address, device_uuid, network_id, created_at, completed_at FROM pending_devices"
    )?;

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let mac_address: String = row.get(1)?;
        let uuid_str: Option<String> = row.get(2)?;
        let network_id: i64 = row.get(3)?;
        let created_at: String = row.get(4)?;
        let completed_at: Option<String> = row.get(5)?;

        let device_uuid = uuid_str
            .map(|s| Uuid::parse_str(&s))
            .transpose()
            .map_err(|e| anyhow::anyhow!("Failed to parse UUID in pending_devices: {}", e))?;

        conn.execute(
            "INSERT INTO pending_devices_new VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                mac_address,
                device_uuid,
                network_id,
                created_at,
                completed_at
            ],
        )?;
    }

    Ok(())
}

fn finalize_migration(conn: &Connection) -> Result<()> {
    log::debug!("Finalizing migration: replacing old tables with new tables...");

    conn.execute_batch(
        "-- Drop old tables
         DROP TABLE devices;
         DROP TABLE plans;
         DROP TABLE lifecycle_transitions;
         DROP TABLE dhcp_leases;
         DROP TABLE pending_devices;

         -- Rename new tables
         ALTER TABLE devices_new RENAME TO devices;
         ALTER TABLE plans_new RENAME TO plans;
         ALTER TABLE lifecycle_transitions_new RENAME TO lifecycle_transitions;
         ALTER TABLE dhcp_leases_new RENAME TO dhcp_leases;
         ALTER TABLE pending_devices_new RENAME TO pending_devices;

         -- Recreate indexes for devices
         CREATE INDEX idx_devices_uuid ON devices(uuid);
         CREATE INDEX idx_devices_role_id ON devices(role_id);
         CREATE INDEX idx_devices_architecture ON devices(architecture);

         -- Recreate indexes for plans
         CREATE INDEX idx_plans_device_uuid ON plans(device_uuid);
         CREATE INDEX idx_plans_status ON plans(status);
         CREATE INDEX idx_plans_active ON plans(device_uuid, status) WHERE status IN ('pending', 'running');

         -- Recreate indexes for lifecycle_transitions
         CREATE INDEX idx_lifecycle_transitions_device_uuid ON lifecycle_transitions(device_uuid);
         CREATE INDEX idx_lifecycle_transitions_active ON lifecycle_transitions(device_uuid) WHERE success IS NULL;
         CREATE INDEX idx_lifecycle_transitions_completed ON lifecycle_transitions(device_uuid, completed_at) WHERE success IS NOT NULL;

         -- Recreate indexes for dhcp_leases
         CREATE INDEX idx_dhcp_mac ON dhcp_leases(mac_address);
         CREATE INDEX idx_dhcp_ip ON dhcp_leases(ip_address);
         CREATE INDEX idx_dhcp_state ON dhcp_leases(state);
         CREATE INDEX idx_dhcp_device ON dhcp_leases(device_uuid);
         CREATE INDEX idx_dhcp_leases_network ON dhcp_leases(network_id);

         -- Recreate indexes for pending_devices
         CREATE INDEX idx_pending_devices_mac ON pending_devices(mac_address);
         CREATE INDEX idx_pending_devices_device_uuid ON pending_devices(device_uuid);
         CREATE INDEX idx_pending_devices_completed ON pending_devices(completed_at);

         -- Re-enable foreign keys
         PRAGMA foreign_keys=ON;"
    )?;

    Ok(())
}
