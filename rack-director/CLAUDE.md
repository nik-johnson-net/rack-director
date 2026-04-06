# OVERVIEW

Rack Director is the server component, written in rust. Database functionality is provided by SQLite.

`src/database`: The database initialization code & migrations
`src/dhcp`: DHCP Service, IPAM (IP Address Management) and PXE Boot next-server information.
`src/director`: `Device` code and main business logic.
`src/http`: All HTTP handlers. Divided into `api` for `/api/` routes (currently unused), `cnc` for `/cnc/` routes (devices use this for automation), and `ui` for `/ui/` routes (serves `rack-director-ui`).
`src/lifecycle`: `Lifecycle` and transition code - provisiong states of `Devices` and how they transition from one to the next.
`src/operating_systems`: `Operating System` code, configuration for what operating systems can be installed and how (legacy — being replaced by OSM).
`src/osm`: Operating System Modules — archive parsing, validation, database storage, upload pipeline (streaming upload, validation, extraction, atomic DB replacement), and HTTP API for OSM packages.
`src/plans`: `Plan` code - concrete `Actions` to move a `Device` from one `Lifecycle` state to another.
`src/roles`: `Role` code - configuration for a group of `Devices`, including what `Operating System` to install.
`src/storage`:  Interfaces for the storage layer. Used to store uploaded images for `Operating Systems`.
`src/templates`: Location for storing templates and rendering functions that are needed by other modules.
`tests/`: Integration tests that provide end-to-end testing.

# CODE STYLE

- Update this file if new modules are added or moved.
- ALWAYS run the formatter `cargo fmt` after code changes.
- ALWAYS run the linter `cargo clippy --fix --allow-dirty -p rack-director` after code changes.
- ALWAYS run tests `cargo test` after code changes.

## Test Database Setup Pattern

Use the `test_connection_factory!()` macro for test database setup. Do NOT use `tempfile::tempdir()` + `DatabaseConnectionFactory::new(db_path)` directly.

**Critical rule:** `test_connection_factory!()` uses `stdext::function_name!()` to generate a unique in-memory SQLite URI. It MUST be called inside the test function itself (not in a helper), otherwise all tests share the same URI causing locking conflicts.

**Correct pattern:** Helper functions accept a `DatabaseConnectionFactory` parameter; each test calls `test_connection_factory!()` at the call site and passes it in:

```rust
use crate::{database::{self, DatabaseConnectionFactory}, test_connection_factory};

async fn setup_test_db(factory: DatabaseConnectionFactory) -> database::Connection {
    database::run_migrations(&factory).await.unwrap()
}

#[tokio::test]
async fn test_something() {
    let conn = setup_test_db(test_connection_factory!()).await;
    // ...
}
```

**When the state struct (e.g. `AppState`) holds a `ConnectionFactory`:** The migration connection must be retained alongside the state because dropping it would destroy the in-memory database before `open()` is called again. Return it as an extra value from setup helpers:

```rust
async fn setup_test_state(factory: DatabaseConnectionFactory) -> (Arc<AppState>, TempDir, Connection) {
    let migration_conn = database::run_migrations(&factory).await.unwrap();
    let conn_factory: Arc<dyn ConnectionFactory> = Arc::new(factory);
    let state = Arc::new(AppState { connection_factory: conn_factory, ... });
    (state, temp_dir, migration_conn) // keep migration_conn alive!
}

#[tokio::test]
async fn test_something() {
    let (state, _temp_dir, _migration_conn) = setup_test_state(test_connection_factory!()).await;
    // ...
}
```

**TempDir** is still needed for filesystem paths (agent images, boot files) but NOT for the database path.

## Naming Conventions

- Database connection parameters of type `&Connection` or `&mut Connection` MUST be named `conn`, not `db`.
  ```rust
  // Correct
  pub fn get_device(conn: &Connection, uuid: &str) -> Result<Device> { ... }

  // Wrong
  pub fn get_device(db: &Connection, uuid: &str) -> Result<Device> { ... }
  ```

# System Design

## DATASTORES

- Each module will store its primitives in separate database tables.
- Access to that table will occur in a `store.rs` submodule.
- `store.rs` submodule MUST remain private to the module.

An example `store.rs` module:

```
pub struct ExampleStore {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
}

pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
    Self { conn }
}

pub async fn register_device(&self, uuid: &str, architecture: Architecture) -> Result<()> {
    let conn = self.conn.lock().await;
    conn.execute(
        "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
        params![uuid, architecture.as_str()],
    )?;
    Ok(())
}
```

## Devices

- A Device is any networked chassis that can be booted by rack-director
- Devices are uniquely identified by UUID
- A Device may have multiple NICs and thus multiple MAC addresses
- A Device may have a BMC (Baseboard Management Controller) which will have its own MAC address and IP address, and will not PXE boot.

## Lifecycle

- Devices can be in one of several states:.
- Devices move between states by executing a series a steps in a Plan.
- A Device that has been created in rack-director but not seen on the network is "new".
- A device then seen on the network, or auto-discovered, are then moved to "Unprovisioned" by running steps such as memtest, part enumeration, firmware updates, BMC configuration, etc. At this point the Device is ready to be provisioned in an Infrastructure-as-a-Service (IaaS) manner.
- Provisioning a Device moves it to the Provisioned state, which is accomplished by configuring NICs, disks, and installing an operating system.
- Unprovisioning a Device moves it back to the Unprovisioned state, which can include wiping the disks. 
- A Device can be "Removed", keeping its history but not allowing more actions.

Hardware is, well, hard. Failures can happen at any point. Failures in a transition are handled by moving the device to a "Broken" state, requiring intervention to debug and fix issues. Devices can then be moved back to "Unprovisioned", which will re-run discovery, disk-wipes, etc.

Each Transition is stored in the table called `lifecycle_transitions`

## Actions

- Actions are the underlying instructions for a device to take some action, like reboot, install an OS, or wipe disks.
- Actions can take parameters, useful for configuring login details or what OS to install.
- Actions are organized into Plans, useful for linking back to Lifecycles.

A table called plans is used to store a list of actions, their parameters, and the current step.


# Database Schema

## Overview

Rack Director uses SQLite with 10 migrations. Schema is versioned and migrations are applied sequentially on startup.

**Current Version:** 10 (as of 2026-01)

**Migration Location:** `src/database/migrations/*.sql`

## Core Tables

### devices

Tracks all devices (servers) in the rack.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `uuid` | TEXT | Device UUID (from SMBIOS), unique |
| `created_at` | DATETIME | When device was first created |
| `first_seen_at` | DATETIME | First network appearance |
| `last_seen_at` | DATETIME | Last network appearance |
| `lifecycle` | TEXT | Current lifecycle state (new, unprovisioned, provisioned, removed, broken) |
| `architecture` | TEXT | CPU architecture (x86-64) |
| `role_id` | INTEGER | FK to roles table |
| `attributes` | JSONB | Device metadata (hardware info, network interfaces, disks, etc.) |

**Indexes:** `uuid`, `role_id`, `architecture`

**Migration:** v1 (base), v3 (lifecycle), v5 (role_id, architecture)

### plans

Execution plans that move devices through lifecycle transitions.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `device_uuid` | TEXT | FK to devices(uuid) |
| `status` | TEXT | pending, running, success, failed |
| `current_step` | INTEGER | Current action index (0-based) |
| `total_steps` | INTEGER | Total number of actions |
| `actions` | JSONB | Array of Action objects |
| `error_message` | TEXT | Error message if failed |
| `created_at` | DATETIME | Plan creation time |
| `started_at` | DATETIME | Plan execution start time |
| `completed_at` | DATETIME | Plan completion time |

**Indexes:** `device_uuid`, `status`, `(device_uuid, status)` for active plans

**Migration:** v2

### lifecycle_transitions

History of device lifecycle state changes.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `device_uuid` | TEXT | FK to devices(uuid) |
| `from_state` | TEXT | Starting lifecycle state |
| `to_state` | TEXT | Target lifecycle state |
| `plan_id` | INTEGER | FK to plans(id), nullable |
| `created_at` | DATETIME | Transition start time |
| `completed_at` | DATETIME | Transition completion time |
| `success` | BOOLEAN | Whether transition succeeded |
| `error_message` | TEXT | Error message if failed |

**Indexes:** `device_uuid`, active transitions (`WHERE success IS NULL`), completed transitions

**Migration:** v3

## Role & OS Tables

### operating_systems

Operating system definitions.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `name` | TEXT | OS name (e.g., "Ubuntu") |
| `version` | TEXT | OS version (e.g., "22.04") |
| `description` | TEXT | Human-readable description |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Unique:** `(name, version)`

**Migration:** v5

### os_architectures

Architecture-specific OS configurations (kernel, initramfs, cmdline).

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `os_id` | INTEGER | FK to operating_systems(id) |
| `architecture` | TEXT | CPU architecture (x86-64) |
| `kernel_path` | TEXT | Path to kernel image |
| `initramfs_path` | TEXT | Path to initramfs image |
| `kernel_filename` | TEXT | Original kernel filename |
| `initramfs_filename` | TEXT | Original initramfs filename |
| `modules` | TEXT | JSON array of module paths |
| `cmdline_args` | TEXT | Base kernel cmdline arguments |
| `install_script_path` | TEXT | Path to install script |
| `install_script_filename` | TEXT | Original install script filename |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Unique:** `(os_id, architecture)`

**Migration:** v5, v6 (added filename columns)

### roles

Device role definitions (groups of devices with same configuration).

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `name` | TEXT | Role name (unique) |
| `description` | TEXT | Human-readable description |
| `os_id` | INTEGER | FK to operating_systems(id) |
| `disk_layout` | TEXT | JSON disk partition layout |
| `cmdline_args` | TEXT | Role-specific kernel cmdline args |
| `config_template` | TEXT | JSON additional configuration |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Indexes:** `name`, `os_id`

**Migration:** v5, v10 (added cmdline_args)

## DHCP Tables

### dhcp_networks

DHCP network configurations (multi-network support with relay agents).

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `name` | TEXT | Network name (unique) |
| `subnet` | TEXT | Subnet CIDR (e.g., "10.0.0.0/24") |
| `gateway` | TEXT | Default gateway IP |
| `dns_servers` | TEXT | JSON array of DNS server IPs |
| `lease_duration` | INTEGER | Lease duration in seconds |
| `relay_agent_address` | TEXT | Relay agent IP (for remote networks) |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Indexes:** `relay_agent_address`

**Migration:** v4, v8 (multi-network support)

### dhcp_pools

DHCP IP address pools (ranges) within networks.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `network_id` | INTEGER | FK to dhcp_networks(id) |
| `name` | TEXT | Pool name |
| `range_start` | TEXT | Starting IP address |
| `range_end` | TEXT | Ending IP address |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Unique:** `(network_id, name)`

**Migration:** v8

### dhcp_static_reservations

Static MAC-to-IP reservations.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `network_id` | INTEGER | FK to dhcp_networks(id) |
| `mac_address` | TEXT | MAC address |
| `ip_address` | TEXT | Reserved IP address |
| `hostname` | TEXT | Hostname for reservation |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Unique:** `(network_id, mac_address)`, `(network_id, ip_address)`

**Migration:** v8

### dhcp_leases

Active DHCP leases.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `mac_address` | TEXT | MAC address (unique) |
| `ip_address` | TEXT | Assigned IP address |
| `device_uuid` | TEXT | FK to devices(uuid), nullable |
| `network_id` | INTEGER | FK to dhcp_networks(id) |
| `lease_start` | DATETIME | Lease start time |
| `lease_end` | DATETIME | Lease expiration time |
| `state` | TEXT | offered, active, expired, released |
| `hostname` | TEXT | Requested hostname |
| `created_at` | DATETIME | Creation time |
| `updated_at` | DATETIME | Last update time |

**Indexes:** `mac_address`, `ip_address`, `state`, `device_uuid`, `network_id`

**Migration:** v4, v8 (added network_id)

### pending_devices

Devices with DHCP leases but not yet registered.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Primary key |
| `mac_address` | TEXT | MAC address (unique) |
| `device_uuid` | TEXT | FK to devices(uuid), nullable until boot |
| `network_id` | INTEGER | FK to dhcp_networks(id) |
| `created_at` | DATETIME | Pending device creation time |
| `completed_at` | DATETIME | When device registered |

**Indexes:** `mac_address`, `device_uuid`, `completed_at`

**Migration:** v9

## Schema Relationships

```
devices
  ├─► role_id → roles
  │                ├─► os_id → operating_systems
  │                │              └─► os_architectures
  │                └─► disk_layout (JSON)
  │
  ├─► uuid ← plans (device_uuid)
  │             └─► actions (JSONB array)
  │
  ├─► uuid ← lifecycle_transitions (device_uuid)
  │             └─► plan_id → plans
  │
  └─► uuid ← dhcp_leases (device_uuid)
                  └─► network_id → dhcp_networks
                                      ├─► dhcp_pools
                                      └─► dhcp_static_reservations

pending_devices
  ├─► network_id → dhcp_networks
  └─► device_uuid → devices (after registration)
```

## Recent Schema Changes

### Migration v10 (2026-01, commit 7c6b810)
- Added `cmdline_args` column to `roles` table
- Enables role-level kernel cmdline configuration
- Part of 3-tier cmdline merging system (OS → Role → Device)

### Migration v9 (2025-12)
- Added `pending_devices` table
- Supports lease-based device creation workflow
- Tracks devices before they're fully registered

### Migration v8 (2025-11)
- Multi-network DHCP support
- Added `dhcp_networks`, `dhcp_pools`, `dhcp_static_reservations` tables
- Relay agent support for managing multiple subnets
- Migrated old `dhcp_config` to "Default" network

## Querying the Database

### Get Device with Role and OS
```sql
SELECT d.*, r.name as role_name, os.name as os_name, os.version as os_version
FROM devices d
LEFT JOIN roles r ON d.role_id = r.id
LEFT JOIN operating_systems os ON r.os_id = os.id
WHERE d.uuid = ?;
```

### Get Active Plans for Device
```sql
SELECT * FROM plans
WHERE device_uuid = ?
AND status IN ('pending', 'running')
ORDER BY created_at DESC;
```

### Get Lifecycle History
```sql
SELECT * FROM lifecycle_transitions
WHERE device_uuid = ?
ORDER BY created_at DESC;
```

### Get DHCP Leases for Network
```sql
SELECT l.*, d.uuid as device_uuid
FROM dhcp_leases l
LEFT JOIN devices d ON l.device_uuid = d.uuid
WHERE l.network_id = ?
AND l.state = 'active';
```

# FAQ

## How do I perform a migration?

There must only ever be one migration file for a git branch.

- Find the next number in sequence.
- Write the migration to `src/database/migrations/{next}.sql`.
- Append to the `MIGRATIONS` variable in `src/database/mod.rs`: `include_str!("migrations/{next}.sql")`
- Migrations run automatically on startup
- No rollback support - test migrations carefully

## How do I query device attributes?

Device attributes are stored as JSONB in the `attributes` column. Use SQLite's JSON functions:

```sql
-- Get specific attribute
SELECT json_extract(attributes, '$.manufacturer') FROM devices WHERE uuid = ?;

-- Filter by attribute
SELECT * FROM devices WHERE json_extract(attributes, '$.product_name') = 'PowerEdge R740';

-- Check if attribute exists
SELECT * FROM devices WHERE json_extract(attributes, '$.bmc_ip') IS NOT NULL;
```

## How are kernel cmdline arguments merged?

See `src/lifecycle/CLAUDE.md` — "Configuration Hierarchies" section. Three tiers: OS → Role → Device, merged by `src/templates/mod.rs::merge_cmdline_args()`.
