# UI HTTP Module

This module (`src/http/ui`) serves the rack-director-ui React frontend and exposes all `/ui/` REST API endpoints consumed by it. Static asset serving and SPA fallback routing are handled in `mod.rs`; each resource type lives in its own submodule.

## Module Structure

| File | Responsibility |
|------|----------------|
| `mod.rs` | Route registration, static assets (`/ui/assets/{asset}`), SPA index fallback |
| `devices.rs` | Device CRUD, lifecycle transitions, platform/role assignment, pending devices |
| `networks.rs` | DHCP networks, pools, static reservations, per-network lease listing, make-static |
| `dhcp.rs` | Global DHCP lease queries (all leases, by MAC) |
| `operating_systems.rs` | OS and architecture CRUD, kernel/initramfs/module/install-script upload & download |
| `platforms.rs` | Platform CRUD, platform-device listing |
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
| GET | `/ui/devices/{uuid}/transitions` | List all transitions |
| GET | `/ui/devices/{uuid}/transitions/active` | Get active transition |
| GET | `/ui/devices/{uuid}/status` | Get current plan step/status |
| POST | `/ui/devices/{uuid}/platform` | Assign platform to device |
| GET | `/ui/devices/{uuid}/platform` | Get device's platform |
| POST | `/ui/devices/{uuid}/role` | Assign role to device |
| GET | `/ui/devices/{uuid}/role` | Get device's role |
| POST | `/ui/devices/pending` | Create pending device |
| GET | `/ui/devices/pending` | List pending devices |
| DELETE | `/ui/devices/pending/{id}` | Delete pending device |

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

### Operating Systems (`operating_systems.rs`)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/ui/operating_systems` | Create OS |
| GET | `/ui/operating_systems` | List OSes |
| GET | `/ui/operating_systems/{id}` | Get OS |
| PUT | `/ui/operating_systems/{id}` | Update OS |
| DELETE | `/ui/operating_systems/{id}` | Delete OS |
| POST | `/ui/operating_systems/{id}/architectures` | Create OS architecture |
| GET | `/ui/operating_systems/{id}/architectures/{arch}` | Get OS architecture |
| DELETE | `/ui/operating_systems/{id}/architectures/{arch}` | Delete OS architecture |
| POST | `/ui/operating_systems/{id}/architectures/{arch}/kernel` | Upload kernel image |
| POST | `/ui/operating_systems/{id}/architectures/{arch}/initramfs` | Upload initramfs |
| POST | `/ui/operating_systems/{id}/architectures/{arch}/modules` | Upload kernel module |
| POST | `/ui/operating_systems/{id}/architectures/{arch}/install_script` | Upload install script |
| GET | `/ui/operating_systems/{id}/architectures/{arch}/download/{component}` | Download component |

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
