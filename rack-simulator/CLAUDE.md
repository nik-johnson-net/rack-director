# rack-simulator Documentation

## Overview

rack-simulator is a development and testing tool that simulates server hardware interactions with rack-director. It has two primary use cases:

1. **Manual simulation** – Simulate individual protocol steps (DHCP, TFTP, iPXE, agent) via CLI commands, useful for interactive development against a running rack-director.
2. **Automated QEMU-based E2E tests** – Spawn real director and agent VMs over a virtual network to test the full provisioning lifecycle end-to-end.

## Architecture

### Source Structure

```
rack-simulator/src/
├── main.rs               # CLI entry point and subcommand routing
├── config.rs             # Server config (TOML), architecture enum, hardware profile merging
├── server.rs             # ServerState – persistent DHCP/boot state per simulated server
├── dhcp.rs               # DHCP DISCOVER/REQUEST implementation
├── tftp.rs               # TFTP RRQ/DATA download simulation
├── http.rs               # HTTP client for rack-director API (iPXE, attributes, actions)
├── boot.rs               # Boot sequence orchestration and iPXE chain following
├── agent.rs              # Agent simulation – hardware attribute upload
├── output.rs             # Formatted console output (step, detail, success, error)
├── hardware_profiles.rs  # Pre-defined hardware configs (Dell R640, HPE DL380, etc.)
├── vm/
│   ├── mod.rs
│   └── qemu.rs           # QEMU process spawn, arg builders, disk image creation
└── e2e/
    ├── mod.rs
    ├── runner.rs          # Test orchestration (single, sequential, parallel)
    ├── config.rs          # TOML test configuration parsing
    ├── director.rs        # DirectorVm – API client for setting up rack-director
    ├── agent_vm.rs        # AgentVm – spawns PXE-boot QEMU VMs
    ├── lifecycle.rs       # Device lifecycle state machine driver
    └── build.rs           # Docker image builds + Rocky Linux installer download
```

### Key Data Flow

```
rack-simulator boot <server>
    └── boot.rs::full_boot()
            ├── dhcp.rs::discover_all_nics()      → DHCP DISCOVER/OFFER (firmware)
            ├── tftp.rs::download()               → TFTP bootloader download
            ├── dhcp.rs::request_as_ipxe()        → iPXE DHCP REQUEST
            ├── http.rs::get_ipxe_script()        → iPXE script from /cnc/ipxe
            └── agent.rs::run()                   → Hardware attributes POST + action_success

rack-simulator e2e run <test.toml>
    └── runner.rs::run_test()
            ├── build.rs::ensure_*_images()       → Docker builds (cached)
            ├── director.rs::start()              → Director QEMU VM + API setup
            ├── agent_vm.rs::start()              → Agent QEMU VM (PXE boots)
            └── lifecycle.rs::drive_lifecycle()   → Poll state, trigger transitions
```

## CLI Subcommands

```bash
# Full boot cycle (DHCP → TFTP → iPXE → agent)
cargo run --bin rack-simulator -- boot <server>

# Individual protocol steps
cargo run --bin rack-simulator -- dhcp-discover <server>
cargo run --bin rack-simulator -- dhcp-request <server>
cargo run --bin rack-simulator -- tftp-download <server>
cargo run --bin rack-simulator -- ipxe-boot <server>
cargo run --bin rack-simulator -- agent-run <server>

# Server config management
cargo run --bin rack-simulator -- config create-server <name> [--arch x64-uefi] [--profile dell-r640]
cargo run --bin rack-simulator -- config list
cargo run --bin rack-simulator -- config remove-server <name>

# E2E tests
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml
cargo run --bin rack-simulator -- e2e run-all e2e-tests/ [--parallel]
```

**Default ports:** DHCP 1067, TFTP 1069, HTTP 3000 (all on localhost)

## Configuration

### Server Config (`~/.config/rack-simulator/config.toml`)

```toml
[[servers]]
name = "test-server-1"
architecture = "x64-uefi"          # x86-bios | x64-uefi | arm64-uefi | x64-uefi-http
hardware_profile = "dell-r640"     # dell-r640 | dell-r750 | hp-dl380 | supermicro-x12 | generic
mac_addresses = ["52:54:00:12:34:56", "52:54:00:12:34:57"]
uuid = "550e8400-e29b-41d4-a716-446655440000"

[servers.bmc]
mac_address = "52:54:00:12:34:ff"
ip_source = "DHCP"                 # DHCP | Static
```

**Auto-generation:** If `mac_addresses` or `uuid` are omitted, they are deterministically generated from the server name using DJB2 hashing.

**Backward compatibility:** A single `mac_address` field expands to two sequential MACs.

### Server State (`~/.cache/rack-simulator/<name>.state.json`)

Persists DHCP-allocated IPs, boot parameters, and BMC state between invocations. State is automatically updated as boot steps complete.

### Architecture Codes (DHCP Option 93)

| Architecture | Code | Boot method |
|---|---|---|
| `x86-bios` | 0 | TFTP |
| `x64-uefi` | 7 | TFTP |
| `arm64-uefi` | 11 | TFTP |
| `x64-uefi-http` | 15 | HTTP (skips TFTP) |

### Hardware Profiles

Pre-defined profiles available: `dell-r640`, `dell-r750`, `hp-dl380`, `supermicro-x12`, `generic`.

Each profile specifies CPU, memory DIMMs, disks (NVMe/SSD/HDD), and NIC configuration. Profile values are merged with per-server overrides.

## E2E Tests

### Test Config Format (`e2e-tests/*.toml`)

```toml
[test]
name = "disk-gpt-simple"
timeout_seconds = 600

[vm]
memory_mb = 1024
disks = [{ size_gb = 20 }]

[rack_director]
[[rack_director.platforms]]
name = "test-platform"
[rack_director.platforms.attributes]
disks = [{ path = "/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0", size_gb = 20, disk_type = "hdd", label = "ROOT" }]
nics = [{ logical = "eth0", speed_mbps = 1000 }]
cpus = [{ brand = "intel", model = "Unknown", cores = 1 }]
memory_gib = 1

[[rack_director.roles]]
name = "test-role"
platform = "test-platform"
[rack_director.roles.disk_layout]
# ... (disk layout config as per role schema)

[lifecycle]
steps = [
  { from = "unprovisioned", to = "provisioning", action = "partition_disks" },
]
expect_final_state = "provisioned"
```

### E2E Test Flow

1. Ensure Docker images are built (cached after first run):
   - `agent-image-export` target → `vmlinuz` + `initramfs`
   - `rack-director-e2e-export` target → `vmlinuz-director` + `director-initramfs.img`
2. Download Rocky Linux 10.1 installer if needed (cached)
3. Start director QEMU VM (wait up to 20 min under TCG)
4. Set up rack-director via API (OS, platforms, roles, DHCP network)
5. Start agent QEMU VM (PXE boots from director)
6. Wait for device to register and reach `unprovisioned` state
7. Assign platform and role
8. Drive lifecycle state machine per `lifecycle.steps`
9. Assert final state matches `expect_final_state`

### Image Locations (`.build/`)

- `.build/agent-image/vmlinuz` and `initramfs`
- `.build/director-image/vmlinuz-director` and `director-initramfs.img`
- `.build/rocky-installer/vmlinuz` and `initrd`
- `.build/<test>-disk-<n>.img` – sparse disk images for agent VMs
- `.build/console-logs/` – serial console output per VM
- `.build/<test>-net0.pcap` – packet capture of agent NIC

**Important:** `.build/` is gitignored and excluded from Docker build context.

## QEMU Networking

VMs communicate over UDP unicast tunnels (not TAP/bridge), which survive guest reboots:

- **Director NIC0** listens on `director_net_port`, sends to `agent_net_port`
- **Agent NIC0** listens on `agent_net_port`, sends to `director_net_port`
- **Director NIC1** uses user networking with `hostfwd` for HTTP port 3000

Packet captures are written to `.build/<name>-net0.pcap` automatically.

## Platform/Host Considerations

### Windows (TCG)

- WHPX is NOT used – it breaks AlmaLinux 10 / Rocky Linux due to incomplete XCR0 emulation (required for AVX2/x86-64-v3 glibc)
- TCG with `-cpu Icelake-Server-noTSX` is used instead
- Director VM boots in ~2–3 minutes under TCG; agent VM takes ~30s

### Linux (KVM)

- KVM acceleration is used when `/dev/kvm` is available
- Standard x86-64 CPU works; no special CPU model required

## Common Issues

### Director VM doesn't start / times out

- Check QEMU is installed and on PATH
- On Windows: ensure QEMU for Windows is installed (not WSL QEMU)
- Director VM needs **2048 MB RAM** minimum (449 MB initramfs loaded into guest RAM at boot)
- Check `.build/console-logs/director.log` for boot errors

### Agent VM can't reach rack-director

- Verify director VM is fully up (HTTP 200 on `/ui/devices`) before agent boots
- Check port numbers in QEMU args (net_port / director_net_port must match)
- Check `.build/<test>-net0.pcap` with Wireshark to inspect DHCP traffic

### Device not appearing in rack-director

- Confirm agent VM booted via PXE (check `.build/console-logs/agent-*.log`)
- Check rack-director DHCP is configured for the 10.0.0.0/24 subnet
- Verify the director's static IP (10.0.0.1) is accessible from agent subnet

### Platform not auto-matched

- Hardware attributes in `PlatformSpec` must match within ±5% disk size and ±1 GiB memory
- Use exact values from agent VM attribute upload – check rack-director device details
- Disk `path` must match the actual virtio device path (empirically verify via serial log)

### Virtio disk paths

Agent VMs use virtio-blk disks. The by-path naming follows the PCI bus topology. For a single disk, the path is typically:

```
/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0
```

Always verify by checking agent serial logs or running `ls /dev/disk/by-path/` in the agent VM.

## Rack-Director API Endpoints Used

| Endpoint | Method | Purpose |
|---|---|---|
| `/ui/devices` | GET | Health check + device list |
| `/ui/operating_systems` | POST | Create OS record |
| `/ui/platforms` | POST | Create platform |
| `/ui/roles` | POST | Create role (use `os_id` not `operating_system_id`) |
| `/ui/dhcp/networks` | POST | Create DHCP subnet/pool |
| `/cnc/ipxe` | GET | Fetch iPXE boot script |
| `/cnc/agent-images/{filename}` | GET | Download agent kernel/initramfs |
| `/cnc/update_attributes` | POST | Upload device hardware attributes |
| `/cnc/action_success` | POST | Report action completion |
| `/cnc/action_failed` | POST | Report action failure |

## See Also

- @.claude/docs/e2e-testing.md – Full E2E test documentation
- @.claude/docs/actions-reference.md – Disk layout format and lifecycle actions
- `rack-agent/CLAUDE.md` – Agent-side documentation
- `e2e-tests/` – Test scenario TOML files
