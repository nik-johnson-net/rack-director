# UI HTTP Module

This module (`src/http/ui`) serves the rack-director-ui React frontend and exposes all `/ui/` REST API endpoints consumed by it. Static asset serving and SPA fallback routing are handled in `mod.rs`; each resource type lives in its own submodule.

## Module Structure

| File | Responsibility |
|------|----------------|
| `mod.rs` | Route registration, static assets (`/ui/assets/{asset}`), SPA index fallback |
| `devices.rs` | Device CRUD, lifecycle transitions, platform/role assignment, pending devices |
| `networks.rs` | DHCP networks, pools, static reservations, per-network lease listing, make-static |
| `dhcp.rs` | Global DHCP lease queries (all leases, by MAC) |
| `platforms.rs` | Platform CRUD, platform-device listing |
| `osm.rs` | OSM module CRUD, upload, export, OS enable/disable |
| `power.rs` | Power status/control endpoints (`/ui/devices/{uuid}/power`) |
| `roles.rs` | Role CRUD, role-device listing |
| `validation.rs` | Shared validation helpers — no routes |

## API Routes

### Devices (`devices.rs`)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ui/devices` | List all devices |
| GET | `/ui/devices/{uuid}` | Get device |
| DELETE | `/ui/devices/{uuid}` | Delete device |
| PATCH | `/ui/devices/{uuid}/attributes` | Update device attributes (hostname, BMC config, cmdline, etc.) |
| GET | `/ui/devices/{uuid}/lifecycle` | Get lifecycle history |
| POST | `/ui/devices/{uuid}/lifecycle/transition` | Start a lifecycle transition |
| POST | `/ui/devices/{uuid}/lifecycle/cancel` | Cancel the active lifecycle transition |
| GET | `/ui/devices/{uuid}/transitions` | List all transitions |
| GET | `/ui/devices/{uuid}/transitions/active` | Get active transition |
| GET | `/ui/devices/{uuid}/status` | Get current plan step/status |
| GET | `/ui/devices/{uuid}/power` | Get live BMC power state — 200 for existing device (degrades to `unknown`/null on BMC failure), 404 for unknown UUID, 500 on DB error |
| POST | `/ui/devices/{uuid}/power` | Issue an OOB power command (`on`/`off`/`cycle`; `off` is a hard/immediate power-off) — 200 on success, 404 unknown UUID, 409 no BMC, 502 BMC error |
| POST | `/ui/devices/{uuid}/platform` | Assign platform to device |
| GET | `/ui/devices/{uuid}/platform` | Get device's platform |
| POST | `/ui/devices/{uuid}/role` | Assign role to device |
| GET | `/ui/devices/{uuid}/role` | Get device's role |
| POST | `/ui/devices/pending` | Create pending device |
| GET | `/ui/devices/pending` | List pending devices |
| DELETE | `/ui/devices/pending/{id}` | Delete pending device |
| GET | `/ui/devices/{uuid}/warnings` | List all warnings for a device |
| DELETE | `/ui/devices/{uuid}/warnings/{warning_id}` | Dismiss a warning |

#### Power endpoint notes

- **`POST .../power` with `action: "off"`** issues a **hard/immediate** power-off
  (BMC `ForceOff` / `ipmitool chassis power off`), not a graceful OS shutdown.
  Hosts frequently run the rack-agent in an initramfs that cannot honor ACPI
  soft-off, so a graceful shutdown can silently hang; a hard off matches the UI
  confirm dialog text and operator expectations.
- **`GET .../power` returns 404 for unknown UUIDs** (consistent with sibling device
  endpoints). For a device that _does_ exist, it always returns `200` — BMC
  failures, missing BMC config, and driver errors all degrade to
  `{ "state": "unknown", "driver": null }` so the UI badge can always render. The
  UI client (`getDevicePower`) degrades any non-ok response to `unknown/null`, so
  it handles the 404 gracefully.
- **`POST .../power` distinguishes 404 vs 409**: a nonexistent UUID → `404 Not Found`;
  a device that exists but has no BMC configured → `409 Conflict`. This allows the
  UI and API callers to surface a meaningful error message in each case.

### DHCP Networks, Pools, Reservations (`networks.rs`)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ui/dhcp/networks` | List networks |
| POST | `/ui/dhcp/networks` | Create network |
| GET | `/ui/dhcp/networks/{id}` | Get network |
| PUT | `/ui/dhcp/networks/{id}` | Update network |
| DELETE | `/ui/dhcp/networks/{id}` | Delete network |
| GET | `/ui/dhcp/networks/{network_id}/pools` | List pools |
| POST | `/ui/dhcp/networks/{network_id}/pools` | Create pool |
| PUT | `/ui/dhcp/pools/{id}` | Update pool |
| DELETE | `/ui/dhcp/pools/{id}` | Delete pool |
| GET | `/ui/dhcp/networks/{network_id}/static-reservations` | List static reservations |
| POST | `/ui/dhcp/networks/{network_id}/static-reservations` | Create static reservation |
| DELETE | `/ui/dhcp/static-reservations/{id}` | Delete static reservation |
| GET | `/ui/dhcp/networks/{network_id}/leases` | List leases for a network |
| POST | `/ui/dhcp/leases/{id}/make-static` | Convert lease to static reservation |

### DHCP Leases (`dhcp.rs`)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ui/dhcp/leases` | List all leases |
| GET | `/ui/dhcp/leases/{mac}` | Get lease by MAC address |

### OSM Modules (`osm.rs`)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ui/osm/modules` | List all OSM modules |
| GET | `/ui/osm/modules/{id}` | Get module details |
| DELETE | `/ui/osm/modules/{id}` | Delete module (blocked if Default) |
| GET | `/ui/osm/modules/{id}/operating-systems` | List OS entries for module |
| GET | `/ui/osm/modules/{id}/export` | Download/export OSM archive |
| POST | `/ui/osm/upload` | Upload OSM archive (async, 10 GiB limit) |
| GET | `/ui/osm/uploads` | List recent uploads |
| GET | `/ui/osm/uploads/{id}` | Get upload status |
| GET | `/ui/osm/operating-systems` | List all OS entries |
| POST | `/ui/osm/operating-systems/{id}/disable` | Disable an OS |
| POST | `/ui/osm/operating-systems/{id}/enable` | Enable an OS |

### Platforms (`platforms.rs`)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/ui/platforms` | Create platform |
| GET | `/ui/platforms` | List platforms |
| GET | `/ui/platforms/{id}` | Get platform |
| PUT | `/ui/platforms/{id}` | Update platform |
| DELETE | `/ui/platforms/{id}` | Delete platform (blocked if devices assigned) |
| GET | `/ui/platforms/{id}/devices` | List platform devices |

### Roles (`roles.rs`)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/ui/roles` | Create role |
| GET | `/ui/roles` | List roles |
| GET | `/ui/roles/{id}` | Get role |
| PUT | `/ui/roles/{id}` | Update role |
| DELETE | `/ui/roles/{id}` | Delete role |
| GET | `/ui/roles/{id}/devices` | List role devices |

## Validation Framework (`validation.rs`)

All endpoints that accept user input use the `ValidationErrors` builder to produce structured HTTP 400 responses:

```json
{ "errors": { "field_name": "Error message" } }
```

### Adding Validation to an Endpoint

```rust
use super::validation::*;

pub fn validate_create_foo(req: &CreateFooRequest) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();
    errors.add_if_err("name", validate_required(&req.name, "Name"));
    errors.add_if_err("name", validate_string_length(&req.name, 255, "Name"));
    errors.into_result()
}

async fn create_foo(...) -> Result<..., HttpError> {
    if let Err(errors) = validate_create_foo(&req) {
        return Err(HttpError::ValidationError(errors));
    }
    // ...
}
```

### Available Validators

| Function | Description |
|----------|-------------|
| `validate_required(value, field_name)` | Non-empty string |
| `validate_string_length(value, max, field_name)` | Max character length |
| `validate_ipv4_address(ip)` | Valid IPv4 address |
| `validate_cidr_subnet(subnet)` | Valid CIDR notation; returns `Ipv4Subnet` on success |
| `validate_ip_in_subnet(ip, subnet)` | IP within a subnet range |
| `validate_ipv4_list(ips, field_name, min_count)` | List of valid IPs with minimum count |
| `validate_u32_range(value, min, max, field_name)` | Numeric range check |
| `validate_hostname(hostname)` | RFC 1123 hostname (alphanum, hyphens, dots; no leading/trailing hyphens) |

Network-specific validators (`validate_create_network_request`, `validate_update_network_request`) live in `validation.rs` and include async duplicate-checking against the database.

## Error Handling

`HttpError` is defined in `src/http/error.rs`. The `ValidationError` variant serializes the field-error map as the response body.
