# End-to-End (E2E) Testing

## Overview

The e2e test system runs full provisioning flows using two real QEMU virtual machines:

- **Director VM** — runs rack-director (DHCP, TFTP, HTTP) on an isolated virtual network
- **Agent VM** — PXE-boots through the director VM and runs rack-agent against virtual disks

Each test exercises the complete flow:
```
agent firmware DHCP → director DHCP → TFTP bootloader → iPXE script → rack-agent → partition disks → verify → report success
```

Tests are driven from the host as ordinary `rack-simulator e2e` commands. No root access is required — DHCP runs on port 67 inside the guest VM.

## Prerequisites

- QEMU (`qemu-system-x86_64`) in PATH
- Docker (for building director VM images — done automatically on first run)
- Agent VM images (already produced by existing Docker build):
  - `.local-storage/agent-image/vmlinuz`
  - `.local-storage/agent-image/initramfs.img`

## Architecture

```
Host
│
├─ rack-simulator e2e run <test.toml>
│    │
│    ├─ Director VM  (QEMU q35)
│    │    NIC0: multicast socket 230.0.0.1:PORT  →  internal network
│    │    NIC1: user hostfwd :PORT → :3000        →  host HTTP access
│    │    Runs: rack-director (DHCP :67, TFTP :69, HTTP :3000)
│    │
│    └─ Agent VM  (QEMU q35)
│         NIC0: multicast socket 230.0.0.1:PORT  →  internal network (PXE)
│         Drives: virtio raw disk images
│         PXE-boots via director, runs rack-agent
│
└─ HTTP polling: host → director VM via hostfwd
```

Each test gets a unique multicast port (range 20000–29999) and TCP port (range 30000–39999), so parallel tests don't interfere.

## Building the Director VM Image

Director VM images are built **automatically** the first time you run any `rack-simulator e2e` command. If `.local-storage/director-image/vmlinuz-director` and `director-initramfs.img` don't exist, the runner will execute:

```
docker build --target rack-director-e2e-export --output .local-storage/director-image .
```

The `rack-director-e2e-export` scratch stage in `docker/Dockerfile` exports exactly those two files from the `director-image-builder` build stage.

The `director-image-builder` stage:
1. Installs a minimal AlmaLinux 10.1 environment with kernel, systemd, and systemd-networkd
2. Copies rack-director binary, agent images, iPXE firmware, and UI static files into the guest filesystem at the paths the binary expects (`/opt/rack-director/...`)
3. Configures static IP `10.0.0.1/24` on NIC0 (matched by MAC `52:54:00:00:00:01`) and DHCP on NIC1 (control/hostfwd NIC)
4. Enables the `rack-director.service` systemd unit
5. Masks NetworkManager (see notes below)
6. Builds a dracut initramfs embedding a squashfs of the guest root filesystem

To build manually (e.g. to pre-warm the cache):
```bash
docker build --target rack-director-e2e-export --output .local-storage/director-image .
```

## Writing Tests

Tests are TOML files in `e2e-tests/`. Each file describes a complete test scenario.

### Test File Structure

```toml
[test]
name = "my-test"
description = "Optional description"
timeout_seconds = 300

[vm]
memory_mb = 512
disks = [{ size_gb = 20 }]   # One entry per virtio disk

[[rack_director.platforms]]
name = "my-platform"
[rack_director.platforms.attributes]
# Hardware attributes that must match what rack-agent reports from this VM
disks = [{ name = "vda", size_gb = 20, disk_type = "hdd", label = "ROOT",
           path = "/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0" }]
nics = [{ speed_mbps = 1000 }]
cpus = [{ designation = "CPU0", cores = 1, threads = 1, speed_mhz = 2000 }]
total_memory_mb = 512

[[rack_director.roles]]
name = "my-role"
platform = "my-platform"

[rack_director.roles.disk_layout]
[[rack_director.roles.disk_layout.disks]]
device = "ROOT"              # Resolved by platform label
partition_table = "gpt"

[[rack_director.roles.disk_layout.disks.partitions]]
label = "efi"
size = "512MiB"
filesystem = "vfat"
flags = ["esp", "boot"]

[[rack_director.roles.disk_layout.disks.partitions]]
label = "root"
size = "rest"
filesystem = "ext4"

[[lifecycle.steps]]
from = "new"
to = "unprovisioned"

[[lifecycle.steps]]
from = "unprovisioned"
to = "provisioned"

[lifecycle]
expect_final_state = "provisioned"
```

### Lifecycle Steps

The `lifecycle.steps` array drives the device through state transitions. The runner:
1. Verifies the device is in the `from` state
2. Calls `POST /ui/devices/{uuid}/lifecycle` with the `to` state
3. Polls until the device reaches `to` (or enters `broken`/`failed`)

### Platform Attributes and Virtio Disk Paths

The platform `attributes` must match what rack-agent actually reports from inside the VM. The most important field is the disk `path`.

**Determining the correct path:**

Virtio disk paths in a q35 QEMU VM depend on PCI slot assignment. To find the correct path:

```bash
# Boot a minimal VM and inspect
qemu-system-x86_64 -machine q35 -nographic -m 512 \
  -kernel .local-storage/agent-image/vmlinuz \
  -initrd .local-storage/agent-image/initramfs.img \
  -append "console=ttyS0" \
  -drive file=/tmp/test.img,if=virtio,format=raw \
  -netdev user,id=net0 -device virtio-net-pci,netdev=net0
# Inside VM:
ls /dev/disk/by-path/
```

Common values for q35 with virtio:
- First disk: `/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0`

### Multiple Disks

Add multiple entries to `vm.disks` and `rack_director.platforms.attributes.disks`:

```toml
[vm]
disks = [{ size_gb = 20 }, { size_gb = 100 }]

[rack_director.platforms.attributes]
disks = [
  { name = "vda", size_gb = 20, label = "ROOT",
    path = "/dev/disk/by-path/pci-0000:04:00.0-virtio-pci-virtio0" },
  { name = "vdb", size_gb = 100, label = "DATA1",
    path = "/dev/disk/by-path/pci-0000:05:00.0-virtio-pci-virtio0" },
]
```

## Running Tests

### Run a Single Test

```bash
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml
```

With custom image paths:
```bash
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml \
  --agent-kernel /path/to/vmlinuz \
  --agent-initramfs /path/to/initramfs.img \
  --director-kernel /path/to/vmlinuz-director \
  --director-initramfs /path/to/director-initramfs.img
```

Serial logs are written automatically to `.build/console-logs/` — no flag required:
```
.build/console-logs/disk-gpt-simple-director-serial.log
.build/console-logs/disk-gpt-simple-agent-serial.log
```

To write logs to a custom directory instead:
```bash
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml \
  --serial-logs-dir /tmp/e2e-logs
```

### Run All Tests Sequentially

```bash
cargo run --bin rack-simulator -- e2e run-all e2e-tests/
```

### Run All Tests in Parallel

```bash
cargo run --bin rack-simulator -- e2e run-all e2e-tests/ --parallel
```

Each parallel test gets its own VMs, multicast group, and TCP port — they are fully isolated.

Exit code is 1 if any test fails.

## Included Tests

### `e2e-tests/disk-gpt-simple.toml`

Tests the most common layout: a single GPT disk with an ESP and an ext4 root partition using the ROOT platform label.

- **VM:** 512 MiB RAM, 1 × 20 GiB virtio disk
- **Layout:** GPT, 512 MiB vfat (ESP), rest ext4
- **Lifecycle:** new → unprovisioned → provisioned

### `e2e-tests/disk-lvm.toml`

Tests an LVM layout: GPT disk with ESP + LVM PV, volume group `vg0` with `root` (ext4) and `data` (xfs) logical volumes.

- **VM:** 512 MiB RAM, 1 × 20 GiB virtio disk
- **Layout:** GPT, 512 MiB vfat (ESP), rest LVM → vg0 → root (10 GiB ext4) + data (rest xfs)
- **Lifecycle:** new → unprovisioned → provisioned

## How the Runner Works

`rack-simulator e2e run <test.toml>` (`e2e/runner.rs`):

1. Load `TestConfig` from TOML
2. Create a `TempDir` for disk images
3. Pick a random multicast port (UDP 20000–29999) and HTTP port (TCP 30000–39999)
4. **Start Director VM** — spawn QEMU with the director initramfs; poll `GET /ui/devices` until ready (up to 20 minutes — TCG on Windows is slow)
5. **Configure rack-director via HTTP:**
   - `POST /ui/operating_systems` → stub OS record
   - `POST /ui/platforms` for each platform spec
   - `POST /ui/roles` for each role spec (with disk_layout JSON)
6. **Start Agent VM** — create raw disk images, spawn QEMU with the agent initramfs on the same multicast group
7. **Wait for device** — poll `GET /ui/devices` until the agent's device-scan completes and the device appears (up to `timeout_seconds`)
8. **Assign platform and role** — `PUT /ui/devices/{uuid}/platform`, `PUT /ui/devices/{uuid}/role`
9. **Drive lifecycle** — for each step, trigger the transition and poll until the device reaches the target state
10. **Verify final state** — assert `lifecycle_state == expect_final_state`
11. VMs are killed on Drop (even on failure)

## Disk Layout Verification in rack-agent

`rack-agent` now always verifies the disk layout after applying it, before reporting success (`partition.rs::verify_disk_layout()`):

1. **Partition tables** — runs `sfdisk --json <device>` on each disk and validates the JSON output
2. **LVM volume groups** — runs `vgs --noheadings` and checks that each configured VG name appears in the output

If verification fails, the agent calls `action_failed` instead of `action_success`, and the device moves to the `broken` state.

This means e2e tests catch not just partitioning crashes but also silent failures where the disk tool exits 0 but left the disk in a bad state.

## Acceleration

The VM module automatically selects acceleration:

| Platform | Acceleration |
|----------|-------------|
| Linux with `/dev/kvm` | `-enable-kvm -cpu host` |
| Windows / other | `-accel tcg -cpu Icelake-Server-noTSX` (software emulation) |

WHPX is not used on Windows even though it is faster. WHPX's XCR0/XSAVE emulation is incomplete (the upstream patch was never merged), which prevents AVX2 from being enabled. CentOS 10's glibc requires x86-64-v3 (needs AVX2 via XCR0.YMM) and panics on WHPX. TCG fully emulates XCR0 and works correctly.

Tests will run on TCG but will be significantly slower. The director VM alone takes ~2–3 minutes to boot under TCG.

## Director VM Networking: Key Findings

### Predictable Network Interface Naming

AlmaLinux 10.1 uses the `rhel-10.0` udev naming scheme. Virtio NICs on a q35 machine receive predictable names like `enp0s2`, `enp0s3` — never `eth0`/`eth1`. This means networkd `.network` file `[Match]` sections **must not rely on `Name=`** for the rack NIC.

`docker/networkd-rack.network` matches by `MACAddress=52:54:00:00:00:01` only (the MAC is hard-coded in the QEMU args). `docker/networkd-control.network` uses `Type=ether` as a catch-all so it applies to any remaining Ethernet interface after the rack config takes precedence (by filename ordering: `10-rack.network` before `20-control.network`).

### NetworkManager Conflicts with systemd-networkd

`NetworkManager` is pulled in as an indirect dependency of `almalinux-release` on AlmaLinux 10.1 and starts automatically. It is unaware of systemd-networkd's configuration and auto-creates DHCP profiles for both NICs. The DHCP attempt on enp0s2 (the static rack NIC) has no server to respond, so after 45 seconds NetworkManager marks the connection failed and **removes the static 10.0.0.1 address**. From that point, rack-director logs `Found local IPs for interface 2: []` and drops all incoming DHCP requests.

Fix: `build-director.sh` masks `NetworkManager.service`, `NetworkManager-dispatcher.service`, and `NetworkManager-wait-online.service` via `systemctl mask`.

### Serial Console

AlmaLinux 10.1 systemd detects `console=ttyS0` in the kernel cmdline and auto-enables `serial-getty@ttyS0.service`, which depends on `dev-ttyS0.device`. In the live-boot overlayfs environment, udev can take 90+ seconds to create this device unit, stalling the boot log for that window.

Fix: `build-director.sh` masks `serial-getty@ttyS0.service` (no interactive console is needed in the director VM) and writes `/etc/systemd/journald.conf.d/serial-console.conf` with `ForwardToConsole=yes`. This ensures all journald output reaches the serial port via `/dev/console` (which always works since the kernel binds ttyS0 at early boot via `console=ttyS0`).

### networkd-wait-online

The director's `rack-director.service` has `After=systemd-networkd-wait-online.service`. Without a drop-in, networkd-wait-online waits for **all** managed interfaces to become online, which can block indefinitely if the control NIC is slow to get a DHCP address.

Fix: `build-director.sh` installs a drop-in at `/etc/systemd/system/systemd-networkd-wait-online.service.d/any.conf` with `--any --timeout=60`, so the service succeeds as soon as the static rack NIC (10.0.0.1/24) comes up, typically within a few seconds.

## Debugging Failed Tests

### Capture Serial Logs

Serial logs are always written to `.build/console-logs/` by default:
```bash
cargo run --bin rack-simulator -- e2e run e2e-tests/disk-gpt-simple.toml
cat .build/console-logs/disk-gpt-simple-director-serial.log
cat .build/console-logs/disk-gpt-simple-agent-serial.log
```

### Check Director VM Readiness Timeout

If the director VM never becomes ready (20 minute timeout), the director image may be broken or QEMU can't find the binary. Confirm QEMU is in PATH:

```bash
which qemu-system-x86_64
```

### Device Never Appears

If the device doesn't appear within the timeout:
- The agent VM's PXE boot failed — capture serial logs to see the boot output
- The platform attributes don't match — the device scan uploaded attributes that don't match any platform, so it stays unassigned and won't progress

### Device Enters `broken` State

Verification failed. Check the serial logs for the `sfdisk` or `vgs` error output from rack-agent.

### Virtio Disk Path Mismatch

If the platform doesn't match but the disk path looks correct, verify empirically:

```bash
# Boot minimal VM and check
qemu-system-x86_64 -machine q35 -nographic -m 256 \
  -kernel .local-storage/agent-image/vmlinuz \
  -initrd .local-storage/agent-image/initramfs.img \
  -append "console=ttyS0 init=/bin/sh" \
  -drive file=/dev/null,if=virtio,format=raw \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0
# At the shell:
ls -la /dev/disk/by-path/
```

## Module Reference

### `rack-simulator/src/vm/qemu.rs`

Low-level QEMU primitives:

| Function | Purpose |
|----------|---------|
| `QemuProcess::spawn(label, args)` | Spawn a QEMU process; kills it on Drop |
| `QemuProcess::is_running()` | Check if process is still alive |
| `director_vm_args(config)` | Build arg list for director VM |
| `agent_vm_args(config)` | Build arg list for agent VM |
| `create_disk_image(path, size_bytes)` | Create a sparse raw disk image |
| `find_available_udp_port(start, end)` | Find a free UDP port in range |
| `find_available_tcp_port(start, end)` | Find a free TCP port in range |
| `acceleration_args()` | Platform-appropriate KVM/WHPX/TCG flags |
| `find_qemu_binary()` | Locate `qemu-system-x86_64` in PATH |

### `rack-simulator/src/e2e/`

| Module | Purpose |
|--------|---------|
| `build.rs` | `ensure_director_images`: auto-build via Docker if images missing |
| `config.rs` | TOML test config (`TestConfig::load`) |
| `director.rs` | Director VM lifecycle + HTTP API calls |
| `agent_vm.rs` | Agent VM startup and disk image creation |
| `lifecycle.rs` | `drive_lifecycle`: step-by-step state driving with polling |
| `runner.rs` | `run_test`, `run_all`, `run_all_parallel` |

## Docker Files Reference

| File | Purpose |
|------|---------|
| `docker/build-director.sh` | Builds director initramfs inside the Docker build stage |
| `docker/rack-director.service` | systemd unit for rack-director in the VM |
| `docker/networkd-rack.network` | Static 10.0.0.1/24; `[Match]` uses `MACAddress=52:54:00:00:00:01` only — no `Name=` because AlmaLinux 10.1 uses predictable names like `enp0s2` |
| `docker/networkd-control.network` | DHCP on all other Ethernet NICs (`Type=ether`); applied to whichever NIC is not matched by the rack config |
