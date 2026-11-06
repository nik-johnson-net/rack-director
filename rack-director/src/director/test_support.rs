//! Test fixtures for provisioning flows, shared by director and HTTP tests.

use uuid::Uuid;

use super::{Architecture, Director};
use crate::database::Connection;
use crate::lifecycle::DeviceLifecycle;
use crate::plans::Plan;

/// An OS config with one x86-64 architecture, opting into cloud-init or not.
pub(crate) fn os_config(
    name: &str,
    release: &str,
    cloud_init: bool,
) -> osm::os_config::OperatingSystemConfig {
    osm::os_config::OperatingSystemConfig {
        name: name.to_string(),
        release: release.to_string(),
        cloud_init,
        architectures: vec![osm::os_config::ArchitectureConfig {
            arch: "x86-64".to_string(),
            kernel: "vmlinuz".to_string(),
            initramfs: "initrd.img".to_string(),
            modules: vec![],
            cmdline: String::new(),
            install_template: "install.ks".to_string(),
        }],
        template_variables: vec![],
    }
}

/// Register an Unprovisioned device whose role runs AlmaLinux 10 from a bundled `Default`
/// module that does (or does not) opt into cloud-init.
///
/// The role cloud-config is stored as given, unvalidated, so tests can set up a role that a
/// save would refuse.
pub(crate) async fn provisionable_device(
    conn: &mut Connection,
    uuid: &Uuid,
    cloud_init: bool,
    role_cloud_config: Option<&str>,
) {
    let module = crate::osm::store::create_module(
        conn,
        "Default",
        "1.0.0",
        "Test",
        "Test",
        "bundled",
        "osm/Default/1.0.0/",
        false,
        None,
    )
    .await
    .unwrap();
    let config = os_config("AlmaLinux", "10", cloud_init);
    crate::osm::store::create_operating_system(conn, module.id, "alma", "AlmaLinux", "10", &config)
        .await
        .unwrap();
    let layout = common::disk_layout::DiskLayout {
        disks: vec![],
        volume_groups: None,
        zfs_pools: None,
        wipe_all_disks: false,
    };
    let role = crate::roles::store::create(
        conn,
        "role",
        None,
        "Default",
        "AlmaLinux",
        "10",
        "x86-64",
        &layout,
        None,
        None,
        None,
        role_cloud_config,
    )
    .await
    .unwrap();

    Director::new(conn)
        .register_device(uuid, Architecture::X86_64)
        .await
        .unwrap();
    crate::lifecycle::store::update_device_lifecycle(conn, uuid, DeviceLifecycle::Unprovisioned)
        .await
        .unwrap();
    super::store::assign_role_to_device(conn, uuid, role.id.unwrap())
        .await
        .unwrap();
}

/// Start the Provision transition and return its plan.
pub(crate) async fn start_provision(conn: &mut Connection, uuid: &Uuid) -> Plan {
    let mut director = Director::new(conn);
    director
        .start_lifecycle_transition(uuid, DeviceLifecycle::Provisioned)
        .await
        .unwrap();
    director
        .get_active_plan_for_device(uuid)
        .await
        .unwrap()
        .expect("provision plan should be active")
}
