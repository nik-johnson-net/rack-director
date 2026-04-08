/// Integration tests for install script template rendering.
///
/// These tests exercise the full `GET /cnc/install_script?uuid={uuid}` flow:
/// device registration via PXE boot, OS and role creation, install script
/// upload, and template rendering with Handlebars.
///
/// Each layout test renders a template against a fully configured rack-director
/// instance and compares the result byte-for-byte against a static expected
/// output file in `tests/fixtures/expected_scripts/`. This ensures that
/// template changes are intentional and explicitly reviewed.
///
/// # Notes
///
/// ## `bios_grub` flag
/// The `bios_grub` flag is used in BIOS-mode GPT layouts to create a dedicated
/// GRUB BIOS boot partition (a small, unformatted partition for GRUB stage 2).
/// It is supported in the disk layout config and passed through as a template
/// variable. The `partition_disks` action in rack-agent sets flags via
/// `parted set <n> <flag> on`, and parted recognises `bios_grub` for this
/// purpose, so it should work at runtime. However, `bios_grub` is not
/// currently documented in `.claude/docs/actions-reference.md`.
///
/// ## No template validation
/// These tests verify Handlebars template interpolation only — they do NOT
/// verify that the generated scripts are valid Kickstart / Autoinstall configs.
mod common;

use std::io::Write as _;

use anyhow::Result;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test MAC addresses — all use the 52:54:00 QEMU/test prefix and are unique
// ---------------------------------------------------------------------------

const MAC_RHEL10_BIOS_SIMPLE: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x01, 0x01];
const MAC_RHEL10_BIOS_LVM: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x01, 0x02];
const MAC_RHEL10_UEFI_SIMPLE: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x01, 0x03];
const MAC_RHEL10_UEFI_LVM: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x01, 0x04];
const MAC_UBUNTU_BIOS_SIMPLE: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x02, 0x01];
const MAC_UBUNTU_BIOS_LVM: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x02, 0x02];
const MAC_UBUNTU_UEFI_SIMPLE: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x02, 0x03];
const MAC_UBUNTU_UEFI_LVM: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0x02, 0x04];

/// Deterministic test UUID derived from a discriminant byte.
fn test_uuid(discriminant: u8) -> Uuid {
    Uuid::parse_str(&format!(
        "550e8400-e29b-41d4-a716-44665544{:02x}{:02x}",
        0xC0, discriminant
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// Test infrastructure helpers
// ---------------------------------------------------------------------------

/// Start rack-director and set up a test network with autodiscovery enabled.
async fn setup_director() -> Result<common::TestRackDirectorHandle> {
    let handle = common::start_rack_director().await?;
    let network_id = common::create_test_network(handle.handle.http_port).await?;
    common::create_test_pool(handle.handle.http_port, network_id).await?;
    handle
        .set_network_autodiscover(
            u16::try_from(network_id).expect("network_id fits in u16"),
            true,
        )
        .await?;
    Ok(handle)
}

/// Build a tar.zst OSM archive in memory for the given OS configuration.
///
/// The archive contains:
/// - `manifest.toml` — module metadata declaring one OS directory
/// - `{os_dir}/OperatingSystem.toml` — OS config with one x86-64 architecture
/// - `{os_dir}/x86-64/vmlinuz` — dummy kernel bytes
/// - `{os_dir}/x86-64/initrd.img` — dummy initramfs bytes
/// - `{os_dir}/x86-64/{fixture_name}` — the actual install script template
fn create_osm_archive(
    os_name: &str,
    os_release: &str,
    os_dir: &str,
    fixture_name: &str,
) -> Result<Vec<u8>> {
    let template_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/install_scripts")
        .join(fixture_name);
    let template_bytes = std::fs::read(&template_path)?;

    let manifest_toml = format!(
        "name = \"{os_name}-Module\"\nversion = \"1.0.0\"\nauthor = \"Test\"\ndescription = \"Test OSM\"\noperating_systems = [\"{os_dir}\"]\n"
    );
    let os_config_toml = format!(
        "name = \"{os_name}\"\nrelease = \"{os_release}\"\n\n[[architectures]]\narch = \"x86-64\"\nkernel = \"x86-64/vmlinuz\"\ninitramfs = \"x86-64/initrd.img\"\ninstall_template = \"x86-64/{fixture_name}\"\n"
    );
    let os_config_path = format!("{os_dir}/OperatingSystem.toml");
    let kernel_path = format!("{os_dir}/x86-64/vmlinuz");
    let initrd_path = format!("{os_dir}/x86-64/initrd.img");
    let template_archive_path = format!("{os_dir}/x86-64/{fixture_name}");

    let files: Vec<(&str, &[u8])> = vec![
        ("manifest.toml", manifest_toml.as_bytes()),
        (&os_config_path, os_config_toml.as_bytes()),
        (&kernel_path, b"dummy-kernel"),
        (&initrd_path, b"dummy-initramfs"),
        (&template_archive_path, template_bytes.as_slice()),
    ];

    build_tar_zst_archive(&files)
}

/// Builds a zstd-compressed tar archive in memory from the given `(path, content)` pairs.
fn build_tar_zst_archive(files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    // Build uncompressed tar into a buffer first.
    let tar_buf: Vec<u8> = Vec::new();
    let mut tar_builder = tar::Builder::new(tar_buf);

    for (path, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder.append_data(&mut header, path, *content)?;
    }

    let tar_bytes = tar_builder.into_inner()?;

    // Compress with zstd.
    let mut compressed = Vec::new();
    let mut encoder = zstd::Encoder::new(&mut compressed, 1)?;
    encoder.write_all(&tar_bytes)?;
    encoder.finish()?;

    Ok(compressed)
}

/// Upload an OSM archive to the running rack-director and wait for processing to complete.
///
/// Posts to `POST /ui/osm/upload` and polls `GET /ui/osm/uploads/{id}` until the
/// status is `"complete"`. Returns the `module_id` from the completed upload record.
async fn upload_osm_module(
    http_port: u16,
    archive_bytes: Vec<u8>,
    module_name: &str,
) -> Result<i64> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://127.0.0.1:{}/ui/osm/upload", http_port))
        .header("content-type", "application/octet-stream")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{module_name}.tar.zst\""),
        )
        .body(archive_bytes)
        .send()
        .await?
        .error_for_status()?;

    let body: serde_json::Value = response.json().await?;
    let upload_id = body["id"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("upload response missing 'id' field: {body}"))?;

    // Poll until the upload finishes processing (up to 10 seconds).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let status_body: serde_json::Value = client
            .get(format!(
                "http://127.0.0.1:{}/ui/osm/uploads/{}",
                http_port, upload_id
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let status = status_body["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();

        match status.as_str() {
            "complete" => {
                let module_id = status_body["module_id"].as_i64().ok_or_else(|| {
                    anyhow::anyhow!("upload complete but 'module_id' is missing: {status_body}")
                })?;
                return Ok(module_id);
            }
            "failed" => {
                let msg = status_body["error_message"]
                    .as_str()
                    .unwrap_or("unknown error");
                return Err(anyhow::anyhow!("OSM upload failed: {msg}"));
            }
            _ => {}
        }

        if std::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "timed out waiting for OSM upload {upload_id} to complete (status: {status})"
            ));
        }
    }
}

/// Create an OSM module and upload it to the running rack-director instance.
async fn setup_os_with_osm(
    http_port: u16,
    os_name: &str,
    os_release: &str,
    os_dir: &str,
    module_name: &str,
    fixture_name: &str,
) -> Result<()> {
    let archive = create_osm_archive(os_name, os_release, os_dir, fixture_name)?;
    upload_osm_module(http_port, archive, module_name).await?;
    Ok(())
}

/// Create a role via the UI API. Returns the role id.
async fn create_role(http_port: u16, body: serde_json::Value) -> Result<i64> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{}/ui/roles", http_port))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let body: serde_json::Value = response.json().await?;
    Ok(body["id"].as_i64().unwrap())
}

/// Assign a role to a device via the UI API.
async fn assign_role(http_port: u16, device_uuid: &Uuid, role_id: i64) -> Result<()> {
    reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/ui/devices/{}/role",
            http_port, device_uuid
        ))
        .json(&json!({ "role_id": role_id }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Fetch the rendered install script for a device. Asserts HTTP 200.
async fn get_install_script(http_port: u16, device_uuid: &Uuid) -> Result<String> {
    let response = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/cnc/install_script?uuid={}",
            http_port, device_uuid
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        200,
        "install_script should return 200 OK"
    );

    Ok(response.text().await?)
}

/// Execute the complete install script test flow:
/// register device → upload OSM module → create role → assign → fetch.
///
/// Returns the rendered template text.
#[allow(clippy::too_many_arguments)]
async fn run_install_script_test(
    http_port: u16,
    dhcp_port: u16,
    mac: [u8; 6],
    device_uuid: Uuid,
    module_name: &str,
    os_name: &str,
    os_version: &str,
    os_dir: &str,
    fixture_name: &str,
    role_name: &str,
    disk_layout: serde_json::Value,
) -> Result<String> {
    common::register_test_device(http_port, dhcp_port, mac, device_uuid).await?;

    setup_os_with_osm(
        http_port,
        os_name,
        os_version,
        os_dir,
        module_name,
        fixture_name,
    )
    .await?;

    let role_body = json!({
        "name": role_name,
        "osm_module": module_name,
        "os_name": os_name,
        "os_release": os_version,
        "os_arch": "x86-64",
        "disk_layout": disk_layout,
        "config_template": null
    });
    let role_id = create_role(http_port, role_body).await?;
    assign_role(http_port, &device_uuid, role_id).await?;

    get_install_script(http_port, &device_uuid).await
}

/// Load the expected script output for a named test case from the fixtures directory.
///
/// Files are stored in `tests/fixtures/expected_scripts/{name}.txt`. Each file
/// contains the exact expected rendered output including all whitespace and newlines.
fn load_expected(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expected_scripts")
        .join(format!("{}.txt", name));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read expected file {:?}: {}", path, e))
}

// ---------------------------------------------------------------------------
// Disk layout builders
// ---------------------------------------------------------------------------

/// RHEL 10 BIOS GPT: biosboot (bios_grub) + /boot (ext4) + / (xfs)
fn rhel10_bios_simple_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "biosboot", "size": "2MiB", "filesystem": null, "mount_point": null, "flags": ["bios_grub"], "volume_group": null },
                { "label": "boot",     "size": "1GiB", "filesystem": "ext4", "mount_point": "/boot", "flags": ["boot"], "volume_group": null },
                { "label": "root",     "size": "rest", "filesystem": "xfs",  "mount_point": "/",     "flags": [],       "volume_group": null }
            ]
        }],
        "volume_groups": null,
        "zfs_pools": null
    })
}

/// RHEL 10 BIOS GPT LVM: biosboot + /boot (ext4) + LVM PV → vg0 (root xfs + swap)
fn rhel10_bios_lvm_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "biosboot", "size": "2MiB", "filesystem": null, "mount_point": null, "flags": ["bios_grub"], "volume_group": null },
                { "label": "boot",     "size": "1GiB", "filesystem": "ext4", "mount_point": "/boot", "flags": ["boot"], "volume_group": null },
                { "label": "lvm",      "size": "rest", "filesystem": null,   "mount_point": null,    "flags": ["lvm"],  "volume_group": "vg0" }
            ]
        }],
        "volume_groups": [{
            "name": "vg0",
            "logical_volumes": [
                { "name": "root", "size": "50G", "filesystem": "xfs",  "mount_point": "/" },
                { "name": "swap", "size": "8G",  "filesystem": "swap" }
            ]
        }],
        "zfs_pools": null
    })
}

/// RHEL 10 UEFI GPT: efi (vfat) + /boot (ext4) + / (xfs)
fn rhel10_uefi_simple_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "efi",  "size": "600MiB", "filesystem": "vfat", "mount_point": "/boot/efi", "flags": ["esp"], "volume_group": null },
                { "label": "boot", "size": "1GiB",   "filesystem": "ext4", "mount_point": "/boot",     "flags": [],      "volume_group": null },
                { "label": "root", "size": "rest",   "filesystem": "xfs",  "mount_point": "/",         "flags": [],      "volume_group": null }
            ]
        }],
        "volume_groups": null,
        "zfs_pools": null
    })
}

/// RHEL 10 UEFI GPT LVM: efi (vfat) + /boot (ext4) + LVM PV → vg0 (root xfs + swap)
fn rhel10_uefi_lvm_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "efi",  "size": "600MiB", "filesystem": "vfat", "mount_point": "/boot/efi", "flags": ["esp"], "volume_group": null },
                { "label": "boot", "size": "1GiB",   "filesystem": "ext4", "mount_point": "/boot",     "flags": [],      "volume_group": null },
                { "label": "lvm",  "size": "rest",   "filesystem": null,   "mount_point": null,        "flags": ["lvm"], "volume_group": "vg0" }
            ]
        }],
        "volume_groups": [{
            "name": "vg0",
            "logical_volumes": [
                { "name": "root", "size": "50G", "filesystem": "xfs",  "mount_point": "/" },
                { "name": "swap", "size": "8G",  "filesystem": "swap" }
            ]
        }],
        "zfs_pools": null
    })
}

/// Ubuntu 24.04 BIOS GPT: biosboot (bios_grub) + /boot (ext4) + / (ext4)
fn ubuntu_bios_simple_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "biosboot", "size": "2MiB", "filesystem": null,   "mount_point": null,    "flags": ["bios_grub"], "volume_group": null },
                { "label": "boot",     "size": "1GiB", "filesystem": "ext4", "mount_point": "/boot", "flags": [],           "volume_group": null },
                { "label": "root",     "size": "rest", "filesystem": "ext4", "mount_point": "/",     "flags": [],           "volume_group": null }
            ]
        }],
        "volume_groups": null,
        "zfs_pools": null
    })
}

/// Ubuntu 24.04 BIOS GPT LVM: biosboot + /boot (ext4) + LVM PV → ubuntu-vg (root ext4 + swap)
fn ubuntu_bios_lvm_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "biosboot", "size": "2MiB", "filesystem": null,   "mount_point": null,    "flags": ["bios_grub"], "volume_group": null },
                { "label": "boot",     "size": "1GiB", "filesystem": "ext4", "mount_point": "/boot", "flags": [],           "volume_group": null },
                { "label": "lvm",      "size": "rest", "filesystem": null,   "mount_point": null,    "flags": ["lvm"],      "volume_group": "ubuntu-vg" }
            ]
        }],
        "volume_groups": [{
            "name": "ubuntu-vg",
            "logical_volumes": [
                { "name": "root", "size": "50G", "filesystem": "ext4", "mount_point": "/" },
                { "name": "swap", "size": "4G",  "filesystem": "swap" }
            ]
        }],
        "zfs_pools": null
    })
}

/// Ubuntu 24.04 UEFI GPT: efi (vfat) + /boot (ext4) + / (ext4)
fn ubuntu_uefi_simple_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "efi",  "size": "512MiB", "filesystem": "vfat", "mount_point": "/boot/efi", "flags": ["esp"], "volume_group": null },
                { "label": "boot", "size": "1GiB",   "filesystem": "ext4", "mount_point": "/boot",     "flags": [],      "volume_group": null },
                { "label": "root", "size": "rest",   "filesystem": "ext4", "mount_point": "/",         "flags": [],      "volume_group": null }
            ]
        }],
        "volume_groups": null,
        "zfs_pools": null
    })
}

/// Ubuntu 24.04 UEFI GPT LVM: efi (vfat) + /boot (ext4) + LVM PV → ubuntu-vg (root ext4 + swap)
fn ubuntu_uefi_lvm_layout() -> serde_json::Value {
    json!({
        "disks": [{
            "device": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1",
            "partition_table": "gpt",
            "partitions": [
                { "label": "efi",  "size": "512MiB", "filesystem": "vfat", "mount_point": "/boot/efi", "flags": ["esp"], "volume_group": null },
                { "label": "boot", "size": "1GiB",   "filesystem": "ext4", "mount_point": "/boot",     "flags": [],      "volume_group": null },
                { "label": "lvm",  "size": "rest",   "filesystem": null,   "mount_point": null,        "flags": ["lvm"], "volume_group": "ubuntu-vg" }
            ]
        }],
        "volume_groups": [{
            "name": "ubuntu-vg",
            "logical_volumes": [
                { "name": "root", "size": "50G", "filesystem": "ext4", "mount_point": "/" },
                { "name": "swap", "size": "4G",  "filesystem": "swap" }
            ]
        }],
        "zfs_pools": null
    })
}

// ---------------------------------------------------------------------------
// RHEL 10 tests
// ---------------------------------------------------------------------------

/// RHEL 10 BIOS simple GPT layout:
/// biosboot (bios_grub) + /boot (ext4) + / (xfs)
///
/// Verifies:
/// - `/boot` and `/` appear as `part` commands with correct `--onpart` values
/// - `biosboot` (no mount_point) does NOT appear as a `part` command
/// - Network variables are interpolated into the script
#[tokio::test]
async fn test_rhel10_bios_simple_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x01);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_RHEL10_BIOS_SIMPLE,
        device_uuid,
        "RHEL-Module",
        "RHEL",
        "10",
        "rhel10",
        "rhel10.ks",
        "rhel10-bios-simple",
        rhel10_bios_simple_layout(),
    )
    .await?;

    let expected = load_expected("rhel10_bios_simple");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// RHEL 10 BIOS LVM layout:
/// biosboot + /boot (ext4) + LVM PV → vg0 (root xfs + swap)
///
/// Verifies:
/// - `/boot` appears as a `part` command
/// - LVM partition (sda3) does NOT appear as a `part` command (has volume_group)
/// - `logvol /` appears with correct vg/lv/fstype
/// - `swap` LV does NOT appear (no mount_point)
#[tokio::test]
async fn test_rhel10_bios_lvm_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x02);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_RHEL10_BIOS_LVM,
        device_uuid,
        "RHEL-Module",
        "RHEL",
        "10",
        "rhel10",
        "rhel10.ks",
        "rhel10-bios-lvm",
        rhel10_bios_lvm_layout(),
    )
    .await?;

    let expected = load_expected("rhel10_bios_lvm");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// RHEL 10 UEFI simple layout:
/// efi (vfat, /boot/efi) + /boot (ext4) + / (xfs)
///
/// Verifies all three partitions appear as `part` commands with correct
/// `--fstype` and `--onpart` values (sda1, sda2, sda3).
#[tokio::test]
async fn test_rhel10_uefi_simple_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x03);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_RHEL10_UEFI_SIMPLE,
        device_uuid,
        "RHEL-Module",
        "RHEL",
        "10",
        "rhel10",
        "rhel10.ks",
        "rhel10-uefi-simple",
        rhel10_uefi_simple_layout(),
    )
    .await?;

    let expected = load_expected("rhel10_uefi_simple");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// RHEL 10 UEFI LVM layout:
/// efi (vfat) + /boot (ext4) + LVM PV → vg0 (root xfs + swap)
///
/// Verifies:
/// - efi and /boot appear as `part` commands
/// - `logvol /` appears with correct names and fstype
#[tokio::test]
async fn test_rhel10_uefi_lvm_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x04);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_RHEL10_UEFI_LVM,
        device_uuid,
        "RHEL-Module",
        "RHEL",
        "10",
        "rhel10",
        "rhel10.ks",
        "rhel10-uefi-lvm",
        rhel10_uefi_lvm_layout(),
    )
    .await?;

    let expected = load_expected("rhel10_uefi_lvm");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

// ---------------------------------------------------------------------------
// Ubuntu 24.04 tests
// ---------------------------------------------------------------------------

/// Ubuntu 24.04 BIOS simple layout:
/// biosboot + /boot (ext4) + / (ext4)
///
/// Verifies:
/// - `id: part-biosboot` is present (no format/mount section — no filesystem)
/// - `id: fmt-boot` and `id: fmt-root` appear with `fstype: ext4`
/// - MAC address appears in the network section
#[tokio::test]
async fn test_ubuntu2404_bios_simple_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x11);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_UBUNTU_BIOS_SIMPLE,
        device_uuid,
        "Ubuntu-Module",
        "Ubuntu",
        "24.04",
        "ubuntu2404",
        "ubuntu2404.yaml",
        "ubuntu2404-bios-simple",
        ubuntu_bios_simple_layout(),
    )
    .await?;

    let expected = load_expected("ubuntu2404_bios_simple");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// Ubuntu 24.04 BIOS LVM layout:
/// biosboot + /boot (ext4) + LVM PV → ubuntu-vg (root ext4 + swap)
///
/// Verifies:
/// - `/boot` partition appears with fmt entry
/// - `id: lv-root` and `id: mnt-lv-root` appear with `path: /`
/// - `id: lv-swap` does NOT appear (no mount_point)
#[tokio::test]
async fn test_ubuntu2404_bios_lvm_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x12);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_UBUNTU_BIOS_LVM,
        device_uuid,
        "Ubuntu-Module",
        "Ubuntu",
        "24.04",
        "ubuntu2404",
        "ubuntu2404.yaml",
        "ubuntu2404-bios-lvm",
        ubuntu_bios_lvm_layout(),
    )
    .await?;

    let expected = load_expected("ubuntu2404_bios_lvm");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// Ubuntu 24.04 UEFI simple layout:
/// efi (vfat, /boot/efi) + /boot (ext4) + / (ext4)
///
/// Verifies:
/// - `id: fmt-efi` with `fstype: vfat`
/// - MAC address appears correctly
#[tokio::test]
async fn test_ubuntu2404_uefi_simple_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x13);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_UBUNTU_UEFI_SIMPLE,
        device_uuid,
        "Ubuntu-Module",
        "Ubuntu",
        "24.04",
        "ubuntu2404",
        "ubuntu2404.yaml",
        "ubuntu2404-uefi-simple",
        ubuntu_uefi_simple_layout(),
    )
    .await?;

    let expected = load_expected("ubuntu2404_uefi_simple");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

/// Ubuntu 24.04 UEFI LVM layout:
/// efi (vfat) + /boot (ext4) + LVM PV → ubuntu-vg (root ext4 + swap)
///
/// Verifies:
/// - `id: fmt-efi` with `fstype: vfat`
/// - `id: lv-root` and `path: /`
#[tokio::test]
async fn test_ubuntu2404_uefi_lvm_layout() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0x14);

    let script = run_install_script_test(
        http_port,
        dhcp_port,
        MAC_UBUNTU_UEFI_LVM,
        device_uuid,
        "Ubuntu-Module",
        "Ubuntu",
        "24.04",
        "ubuntu2404",
        "ubuntu2404.yaml",
        "ubuntu2404-uefi-lvm",
        ubuntu_uefi_lvm_layout(),
    )
    .await?;

    let expected = load_expected("ubuntu2404_uefi_lvm");
    assert_eq!(
        script, expected,
        "Script output does not match expected file"
    );

    drop(handle);
    Ok(())
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

/// A device with no role assigned should receive a 404 from install_script.
#[tokio::test]
async fn test_install_script_no_role_returns_404() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;
    let dhcp_port = handle.handle.dhcp_port;
    let device_uuid = test_uuid(0xFE);
    let mac = [0x52u8, 0x54, 0x00, 0xC0, 0xFF, 0x01];

    // Register device but do NOT assign a role
    common::register_test_device(http_port, dhcp_port, mac, device_uuid).await?;

    let response = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/cnc/install_script?uuid={}",
            http_port, device_uuid
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        404,
        "Expected 404 for device with no role; got {}",
        response.status()
    );

    drop(handle);
    Ok(())
}

/// A completely unknown UUID should receive an error from install_script.
/// Note: the current implementation returns 500 (Internal Server Error) rather
/// than 404 because `get_device` maps not-found errors to `ServerInternalError`.
#[tokio::test]
async fn test_install_script_unknown_uuid_returns_error() -> Result<()> {
    let handle = setup_director().await?;
    let http_port = handle.handle.http_port;

    let unknown_uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    let response = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/cnc/install_script?uuid={}",
            http_port, unknown_uuid
        ))
        .send()
        .await?;

    assert!(
        response.status().is_client_error() || response.status().is_server_error(),
        "Unknown UUID should return an error status; got {}",
        response.status()
    );

    drop(handle);
    Ok(())
}
