# Platforms

## Overview

Platforms are a primitive in Rack Director grouping similar physical Devices together. They represent common hardware
configurations across Devices and are used to feed Roles with abstracted information for configuration. A Device is
assigned a Platform on discovery via autodetection.

## Platform Attributes

A Platform contains many attributes:

```yaml
disks:
  - path: {from /dev/disk/by-path/}
    size_gb: {size_gb}
    type: {nvme, ssd, hdd}
nics:
  - logical: eno1
    speed_mbps: {speed in Mbps}
cpus:
  - brand: intel
    model: E3-1240 v3
    cores: 8
memory_gib: 32
```

A good source of this information is to run `lshw -json` and search for classes of network, memory, cpu, disk, volume. Before
a platform can be used however, it's disks and NICs must be labeled so the provisioning process can use them. Labels are arbitrary,
but common labels are "ROOT", "DATA1", "DATA2", "NIC1", "NIC2", etc.

## Auto Detection

Platforms are autodetected by the rack-agent during discovery. The rack-agent will discover platform attributes with lshw and
then send them to rack-director. Rack Director will either match with an existing platform or create a new one, and assign the
platform to the device. Labels are also auto-assigned to disks and nics. Disks will choose the smallest, fastest disk as the ROOT
label, and all other disks will get DATA1, DATA2, etc in bus order. NICs will assign in bus order, starting with NIC1.

## Usage in Roles

Roles provide a specific disk layout and additional configuration for the OS. Roles will define disk layouts in terms of labels,
and when the template is rendered for a device, it's platform will be used to replace usage of labels with paths.


```
mount / disk/by-path/{{ disks.ROOT }}
network --dev {{ nics.NIC1 }}
```

will be rendered as

```
mount / disk/by-path/pci-0000:00:1f.2-ata-1
network --dev eno1
```

## Implementation Details

### Database Schema

**Table: platforms**
```sql
CREATE TABLE platforms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    description TEXT,
    attributes BLOB NOT NULL DEFAULT '{}', -- JSON: PlatformAttributes
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

**devices.platform_id:** Nullable foreign key to platforms table (NULL for unassigned devices)

### Auto-Detection Algorithm

**Implementation:** `platforms/detection.rs::detect_or_create_platform()`

When a device completes hardware discovery:

1. **Extract Hardware Attributes**: CPU, memory, disk (path, size, type), NIC (logical name, speed)
   - `convert_device_hardware_to_platform()` - Converts raw device data to platform format
2. **Assign Labels**: Apply heuristics to determine disk/NIC roles
   - `assign_disk_labels()` - ROOT = smallest+fastest, others = DATA1, DATA2, ...
   - `assign_nic_labels()` - NIC1, NIC2, ... in bus order
3. **Find Matching Platform**: Search existing platforms with tolerance
   - `find_matching_platform()` - Iterates through all platforms
   - `is_platform_match()` - Applies matching rules with tolerances:
     - Same disk count and types
     - Disk sizes within ±5% tolerance
     - Same NIC count and speeds (±10% if specified)
     - Same CPU configuration (brand, model, cores)
     - Memory within ±1 GiB tolerance
4. **If Match Found**: Return existing platform ID
5. **If No Match**: Create new platform with auto-generated name, return new platform ID

### Platform Matching Algorithm

Platforms match based on **hardware characteristics**, not bus paths. This allows
identical hardware in different physical slots to be recognized as the same platform.

#### Disk Matching

Disks are sorted in **canonical order** before comparison:
1. **Disk type priority**: NVMe (1) < SSD (2) < HDD (3)
2. **Size**: Smaller disks first
3. **Path**: Tiebreaker only (lexicographic)

This ensures the smallest, fastest disk is always ROOT, regardless of which
physical slot the server occupies.

**Example:**
- Server A: 480GB SSD at slot 0 + 2TB HDD at slot 1 → Platform X
- Server B: 480GB SSD at slot 3 + 2TB HDD at slot 4 → Platform X (same!)

Both servers match Platform X because canonical ordering produces:
- Position 0: 480GB SSD → ROOT
- Position 1: 2TB HDD → DATA1

#### Tolerance Values

- **Disk size**: ±5% (handles manufacturer variations like 480GB vs 500GB)
- **Memory**: ±1 GiB (allows for reserved memory, firmware overhead)
- **NIC speed**: ±10% (handles driver reporting variations)

These tolerances allow minor variations in reported hardware while still
ensuring platforms group truly identical configurations.

### Label Auto-Assignment

**Disks:**
- **ROOT**: Smallest + fastest disk (priority: nvme > ssd > hdd)
- **DATA1, DATA2, ...**: Remaining disks in canonical order (sorted by disk type, then size, then path)

**NICs:**
- **NIC1, NIC2, ...**: NICs in bus order (sorted by logical name)

### REST API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/ui/platforms` | Create new platform |
| GET | `/ui/platforms` | List all platforms |
| GET | `/ui/platforms/{id}` | Get platform details |
| PUT | `/ui/platforms/{id}` | Update platform |
| DELETE | `/ui/platforms/{id}` | Delete platform (prevented if devices assigned) |
| GET | `/ui/platforms/{id}/devices` | List devices on platform |
| POST | `/ui/devices/{uuid}/platform` | Manually assign platform to device |
| GET | `/ui/devices/{uuid}/platform` | Get device's platform |

### Template Label Resolution

When rendering disk layouts or templates for a device:
1. Check if device has assigned platform
2. Resolve labels (ROOT, DATA1, NIC1) to actual device paths from platform attributes
3. If device has no platform or platform missing required label: **fail with clear error**

Example disk layout with labels:
```json
{
  "partitions": [
    {
      "device": "ROOT",  // Will resolve to /dev/disk/by-path/...
      "size": "512MB",
      "filesystem": "vfat",
      "mount_point": "/boot/efi",
      "flags": ["esp", "boot"]
    },
    {
      "device": "DATA1",  // Will resolve to /dev/disk/by-path/... (stable path)
      "size": "rest",
      "filesystem": "xfs",
      "mount_point": "/data",
      "flags": []
    }
  ]
}
```

### Manual Platform Assignment

Users can manually assign or override auto-detected platforms via:
- UI: Device detail page → Platform dropdown → "Assign Platform" button
- API: `POST /ui/devices/{uuid}/platform` with `{"platform_id": 123}`

### Platform Deletion

Platforms cannot be deleted if devices are assigned to them. The API returns:
```json
{
  "error": "Cannot delete platform: 5 devices are assigned to it"
}
```

Users must reassign or remove devices before deleting a platform.

## Code Organization

The platforms module follows a clear **separation of concerns** with distinct responsibilities:

### Module Structure

- **`rack-director/src/platforms/mod.rs`**: Type definitions (Platform, PlatformAttributes, PlatformDisk, PlatformNic, PlatformCpu, DiskType)
- **`rack-director/src/platforms/store.rs`**: Data access layer (CRUD operations only)
- **`rack-director/src/platforms/detection.rs`**: Business logic layer (detection workflow, matching algorithm, label assignment)
- **`rack-director/src/http/ui/platforms.rs`**: HTTP API endpoints for platforms
- **`rack-director/src/director/mod.rs`**: Integration with hardware discovery workflow

### Architecture Guidelines

**store.rs - Data Access Layer**
- Purpose: Database CRUD operations only
- Functions: `create()`, `get()`, `list()`, `update()`, `delete()`
- Should NOT contain: Business logic, matching algorithms, validation beyond data integrity
- Pattern: Repository pattern - abstracts database operations

**detection.rs - Business Logic Layer**
- Purpose: Platform detection workflow and matching logic
- Functions:
  - `detect_or_create_platform()` - Main orchestration function
  - `find_matching_platform()` - Searches for matching platform using tolerance rules
  - `is_platform_match()` - Core matching algorithm with tolerance comparisons
  - `convert_device_hardware_to_platform()` - Converts device attributes to platform format
  - `assign_disk_labels()`, `assign_nic_labels()` - Label assignment heuristics
  - Helper functions for parsing hardware info
- Should NOT contain: Direct database access (uses store instead)

**Why This Separation?**
- **Testability**: Business logic can be tested independently of database
- **Maintainability**: Clear boundaries make code easier to understand
- **Reusability**: Matching algorithm can be used from multiple places
- **Single Responsibility**: Each module has one clear purpose

### Adding New Functionality

When adding platform-related features, ask:
- Is it database access? → Add to `store.rs`
- Is it matching/detection logic? → Add to `detection.rs`
- Is it a REST API endpoint? → Add to `http/ui/platforms.rs`
- Is it type definitions? → Add to `mod.rs`

**Example: Adding a new matching rule**
- ✅ Add to `is_platform_match()` in `detection.rs`
- ❌ Don't add to `store.rs` (not data access)

**Example: Adding a new database query**
- ✅ Add to `store.rs` as a new method
- ❌ Don't mix matching logic with the query

### Testing Conventions

Tests are organized by module:

**store.rs tests:**
- CRUD operations (create, get, list, update, delete)
- Data integrity constraints
- Error conditions (e.g., delete with devices assigned)
- Database-specific behavior

**detection.rs tests:**
- Platform matching algorithm (`is_platform_match()`)
- Matching with tolerances (disk size ±5%, memory ±1GB)
- Label assignment logic
- Hardware parsing functions
- End-to-end detection workflow

Run tests:
```bash
cargo test -p rack-director platforms::
```

## lshw output

Example lshw output is in @.claude/docs/example-lshw.json