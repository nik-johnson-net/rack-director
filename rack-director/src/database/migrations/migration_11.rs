//! Migration 11: Convert UUID columns from TEXT to BLOB (16 bytes binary)
//!
//! This module handles the data migration for converting TEXT UUIDs to binary BLOBs.
//! The table schemas are created by 11.sql, and this code populates them.

use anyhow::Result;
use uuid::Uuid;

use crate::database::Connection;

/// Post-migration hook for migration 11: Convert TEXT UUIDs to BLOB
/// Reads TEXT UUIDs from old tables and inserts as BLOB into new tables
pub async fn convert_uuids(conn: &Connection) -> Result<()> {
    log::info!("Converting TEXT UUIDs to BLOB format...");

    convert_devices_table(conn).await?;
    convert_plans_table(conn).await?;
    convert_lifecycle_transitions_table(conn).await?;
    convert_dhcp_leases_table(conn).await?;
    convert_pending_devices_table(conn).await?;

    finalize_migration(conn).await?;

    log::info!("UUID conversion complete");
    Ok(())
}

/// Parse a TEXT UUID string into its 16-byte BLOB representation.
fn parse_uuid_bytes(uuid_str: &str, context: &str) -> Result<Vec<u8>> {
    let uuid = Uuid::parse_str(uuid_str).map_err(|e| {
        anyhow::anyhow!("Failed to parse UUID '{}' in {}: {}", uuid_str, context, e)
    })?;
    Ok(uuid.into_bytes().to_vec())
}

/// Parse an optional TEXT UUID string into its 16-byte BLOB representation.
fn parse_optional_uuid_bytes(uuid_str: Option<String>, context: &str) -> Result<Option<Vec<u8>>> {
    match uuid_str {
        None => Ok(None),
        Some(s) => parse_uuid_bytes(&s, context).map(Some),
    }
}

async fn convert_devices_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting devices table...");

    type DeviceRow = (
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<i64>,
        String,
    );

    let rows: Vec<DeviceRow> = conn
        .query(
            "SELECT id, uuid, created_at,
                NULLIF(first_seen_at, 0),
                NULLIF(last_seen_at, 0),
                attributes, lifecycle, role_id, architecture FROM devices",
            (),
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .await?;

    for (
        id,
        uuid_str,
        created_at,
        first_seen_at,
        last_seen_at,
        attributes,
        lifecycle,
        role_id,
        architecture,
    ) in rows
    {
        let uuid_bytes = parse_uuid_bytes(&uuid_str, "devices")?;
        conn.execute(
            "INSERT INTO devices_new (id, uuid, created_at, first_seen_at, last_seen_at, attributes, lifecycle, role_id, architecture)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (id, uuid_bytes, created_at, first_seen_at, last_seen_at, attributes, lifecycle, role_id, architecture),
        ).await?;
    }

    Ok(())
}

async fn convert_plans_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting plans table...");

    type PlanRow = (
        i64,
        String,
        String,
        i32,
        i32,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    );

    let rows: Vec<PlanRow> = conn.query(
        "SELECT id, device_uuid, status, current_step, total_steps, actions, error_message, created_at, started_at, completed_at FROM plans",
        (),
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
        )),
    ).await?;

    for (
        id,
        uuid_str,
        status,
        current_step,
        total_steps,
        actions,
        error_message,
        created_at,
        started_at,
        completed_at,
    ) in rows
    {
        let uuid_bytes = parse_uuid_bytes(&uuid_str, "plans")?;
        conn.execute(
            "INSERT INTO plans_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                id,
                uuid_bytes,
                status,
                current_step,
                total_steps,
                actions,
                error_message,
                created_at,
                started_at,
                completed_at,
            ),
        )
        .await?;
    }

    Ok(())
}

async fn convert_lifecycle_transitions_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting lifecycle_transitions table...");

    type TransitionRow = (
        i64,
        String,
        String,
        String,
        Option<i64>,
        String,
        Option<String>,
        Option<bool>,
        Option<String>,
    );

    let rows: Vec<TransitionRow> = conn.query(
        "SELECT id, device_uuid, from_state, to_state, plan_id, created_at, completed_at, success, error_message FROM lifecycle_transitions",
        (),
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
        )),
    ).await?;

    for (
        id,
        uuid_str,
        from_state,
        to_state,
        plan_id,
        created_at,
        completed_at,
        success,
        error_message,
    ) in rows
    {
        let uuid_bytes = parse_uuid_bytes(&uuid_str, "lifecycle_transitions")?;
        conn.execute(
            "INSERT INTO lifecycle_transitions_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                id,
                uuid_bytes,
                from_state,
                to_state,
                plan_id,
                created_at,
                completed_at,
                success,
                error_message,
            ),
        )
        .await?;
    }

    Ok(())
}

async fn convert_dhcp_leases_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting dhcp_leases table...");

    type LeaseRow = (
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        Option<i64>,
    );

    let rows: Vec<LeaseRow> = conn.query(
        "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, created_at, updated_at, network_id FROM dhcp_leases",
        (),
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
        )),
    ).await?;

    for (
        id,
        mac_address,
        ip_address,
        uuid_str,
        lease_start,
        lease_end,
        state,
        hostname,
        created_at,
        updated_at,
        network_id,
    ) in rows
    {
        let device_uuid = parse_optional_uuid_bytes(uuid_str, "dhcp_leases")?;
        conn.execute(
            "INSERT INTO dhcp_leases_new VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
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
                network_id,
            ),
        )
        .await?;
    }

    Ok(())
}

async fn convert_pending_devices_table(conn: &Connection) -> Result<()> {
    log::debug!("Converting pending_devices table...");

    type PendingRow = (i64, String, Option<String>, i64, String, Option<String>);

    let rows: Vec<PendingRow> = conn.query(
        "SELECT id, mac_address, device_uuid, network_id, created_at, completed_at FROM pending_devices",
        (),
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        )),
    ).await?;

    for (id, mac_address, uuid_str, network_id, created_at, completed_at) in rows {
        let device_uuid = parse_optional_uuid_bytes(uuid_str, "pending_devices")?;
        conn.execute(
            "INSERT INTO pending_devices_new VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                id,
                mac_address,
                device_uuid,
                network_id,
                created_at,
                completed_at,
            ),
        )
        .await?;
    }

    Ok(())
}

async fn finalize_migration(conn: &Connection) -> Result<()> {
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
    ).await?;

    Ok(())
}
