# Add Pending Device Page

## Context

The Devices page has an "Add Pending Device" button that links to `/devices/pending/new`, but this page does not exist. Pending devices allow operators to pre-register a device by MAC address before it boots, so rack-director automatically discovers and provisions it when it appears on the network.

Currently the backend requires an active DHCP lease to exist for the MAC address, which prevents pre-staging devices that haven't booted yet. This spec covers both the new UI page and the backend change to remove the lease requirement.

## Design

### Page: `/devices/pending/new`

**Layout:** Standard form page (700px max-width) following existing patterns from NetworkNew, PlatformNew, etc.

**Header:**
- Breadcrumbs: Dashboard > Devices > Add Pending Device
- Title: "Add Pending Device"
- Description: "Pre-register a device by MAC address. It will be automatically discovered when it boots."

**Form:** Single card with two fields:

1. **Network** (required select) - Dropdown of all DHCP networks. Loaded on mount via `getNetworks()`. Selected first because it filters MAC suggestions.

2. **MAC Address** (required text input with autocomplete) - Text input where the user types a MAC address. As they type, a dropdown appears below showing active DHCP leases that:
   - Match the typed prefix (case-insensitive)
   - Belong to the selected network (if one is selected)
   - Don't already have a `device_uuid`
   - Don't already have a pending device entry

   Each suggestion row displays: `MAC — IP address`. Selecting a suggestion fills the MAC field. The user can also type any MAC address freely (for pre-staging before boot).

   The dropdown is hidden when the input is empty, when no suggestions match, or when the input loses focus.

3. **Submit button:** "Add Pending Device" with accent styling. Disabled while submitting.

**After submission:** Navigate to `/devices`.

**Error handling:**
- Form-level error banner (red background) for general errors
- Field-level errors via `useFieldErrors()` hook + `FormFieldError` component
- Backend validation errors (duplicate MAC, invalid format, network not found) displayed per-field

### Backend Change

Remove the DHCP lease validation from `create_pending_device` in `rack-director/src/http/ui/devices.rs`. The handler currently:
1. Looks up a lease by MAC (404 if not found)
2. Checks lease is active (400 if not)
3. Checks lease has no device_uuid (400 if it does)

Replace with:
1. Validate MAC address format (regex: `^([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$`)
2. Validate network exists (query `dhcp_networks` table)
3. Check no existing pending device for this MAC (query `pending_devices` table)
4. Insert into `pending_devices`

Use the existing `ValidationErrors` framework for structured error responses.

### Fetching Lease Suggestions

The page uses `getDhcpLeases()` (already exists in client.ts) to load all leases on mount. Filter client-side by:
- `state === "active"` (or similar)
- `device_uuid` is null/undefined
- `network_id` matches selected network
- `mac_address` starts with typed text

Also load pending devices via `getPendingDevices()` to exclude MACs that are already pending.

## Files

| File | Action | Description |
|------|--------|-------------|
| `rack-director-ui/src/pages/PendingDeviceNew.tsx` | Create | New page component |
| `rack-director-ui/src/main.tsx` | Modify | Add route for `/devices/pending/new` |
| `rack-director/src/http/ui/devices.rs` | Modify | Remove lease requirement, add MAC format + network existence + duplicate validation |

## Verification

1. `cargo test` - Ensure backend tests pass (update existing pending device tests)
2. `npm run build` - Ensure UI compiles
3. Run devserver (`make devserver`) and visit `/devices/pending/new`:
   - Verify form renders with network dropdown and MAC input
   - Type a MAC prefix and verify lease suggestions appear (if leases exist)
   - Submit with a manual MAC address (no lease) and verify it creates successfully
   - Submit with a duplicate MAC and verify field-level error appears
   - Submit with empty fields and verify required field errors appear
   - After success, verify redirect to `/devices`
