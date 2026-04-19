use anyhow::Result;

use super::store::{self, OsmModule, OsmOperatingSystem};
use crate::database::Connection;
use osm::os_config::ArchitectureConfig;

/// Resolved OS data for a role's OS reference.
/// Contains everything needed to serve PXE boot files and render install scripts.
#[derive(Debug, Clone)]
pub struct ResolvedOs {
    /// The module providing this OS.
    pub module: OsmModule,
    /// The OS entry.
    pub os: OsmOperatingSystem,
    /// The specific architecture config.
    pub arch_config: ArchitectureConfig,
}

impl ResolvedOs {
    /// Get the storage path for the kernel file.
    pub fn kernel_storage_path(&self) -> String {
        format!(
            "{}{}/{}",
            self.module.storage_prefix, self.os.dir_name, self.arch_config.kernel
        )
    }

    /// Get the storage path for the initramfs file.
    pub fn initramfs_storage_path(&self) -> String {
        format!(
            "{}{}/{}",
            self.module.storage_prefix, self.os.dir_name, self.arch_config.initramfs
        )
    }

    /// Get the storage path for the install template file.
    pub fn install_template_storage_path(&self) -> String {
        format!(
            "{}{}/{}",
            self.module.storage_prefix, self.os.dir_name, self.arch_config.install_template
        )
    }

    /// Get the cmdline template string.
    pub fn cmdline(&self) -> &str {
        &self.arch_config.cmdline
    }
}

/// Resolve a role's OS reference to concrete OSM data.
///
/// Looks up the module by name, finds the OS entry, and extracts the
/// architecture-specific config.
///
/// # Arguments
/// * `conn` - Database connection
/// * `module_name` - OSM module name (e.g., "Default")
/// * `os_name` - OS name (e.g., "Ubuntu")
/// * `os_release` - OS release (e.g., "22.04")
/// * `arch` - Architecture (e.g., "x86-64")
pub async fn resolve_os(
    conn: &Connection,
    module_name: &str,
    os_name: &str,
    os_release: &str,
    arch: &str,
) -> Result<ResolvedOs> {
    // Find the module
    let module = store::get_module_by_name(conn, module_name)
        .await
        .map_err(|_| anyhow::anyhow!("OSM module '{}' not found", module_name))?;

    // Find the OS entry
    let os = store::get_operating_system_by_name_release(conn, module.id, os_name, os_release)
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "OS '{} {}' not found in module '{}'",
                os_name,
                os_release,
                module_name
            )
        })?;

    // Find the architecture config
    let arch_config = os
        .config
        .architectures
        .iter()
        .find(|a| a.arch.eq_ignore_ascii_case(arch))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Architecture '{}' not found for OS '{} {}' in module '{}'",
                arch,
                os_name,
                os_release,
                module_name
            )
        })?
        .clone();

    Ok(ResolvedOs {
        module,
        os,
        arch_config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_connection_factory;
    use osm::os_config::{ArchitectureConfig, OperatingSystemConfig};

    async fn setup_test_module(conn: &Connection) -> OsmModule {
        let module = store::create_module(
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

        let config = OperatingSystemConfig {
            name: "Ubuntu".to_string(),
            release: "22.04".to_string(),
            architectures: vec![ArchitectureConfig {
                arch: "x86-64".to_string(),
                kernel: "x86-64/vmlinuz".to_string(),
                initramfs: "x86-64/initrd.img".to_string(),
                modules: vec!["x86-64/extra.ko".to_string()],
                cmdline: "console=ttyS0 url={{ install_script_url }}".to_string(),
                install_template: "x86-64/autoinstall.yaml.hbs".to_string(),
            }],
            template_variables: vec![],
        };

        store::create_operating_system(conn, module.id, "ubuntu", "Ubuntu", "22.04", &config)
            .await
            .unwrap();

        module
    }

    #[tokio::test]
    async fn test_resolve_os_success() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        setup_test_module(&conn).await;

        let resolved = resolve_os(&conn, "Default", "Ubuntu", "22.04", "x86-64")
            .await
            .unwrap();

        assert_eq!(resolved.module.name, "Default");
        assert_eq!(resolved.os.name, "Ubuntu");
        assert_eq!(resolved.arch_config.arch, "x86-64");
    }

    #[tokio::test]
    async fn test_resolve_os_case_insensitive() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        setup_test_module(&conn).await;

        let resolved = resolve_os(&conn, "default", "ubuntu", "22.04", "X86-64")
            .await
            .unwrap();

        assert_eq!(resolved.os.name, "Ubuntu");
    }

    #[tokio::test]
    async fn test_resolve_os_unknown_module() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        let result = resolve_os(&conn, "NoSuchModule", "Ubuntu", "22.04", "x86-64").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_resolve_os_unknown_os() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        setup_test_module(&conn).await;

        let result = resolve_os(&conn, "Default", "Windows", "11", "x86-64").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_os_unknown_arch() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        setup_test_module(&conn).await;

        let result = resolve_os(&conn, "Default", "Ubuntu", "22.04", "aarch64").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolved_os_storage_paths() {
        let conn = crate::database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        setup_test_module(&conn).await;

        let resolved = resolve_os(&conn, "Default", "Ubuntu", "22.04", "x86-64")
            .await
            .unwrap();

        assert_eq!(
            resolved.kernel_storage_path(),
            "osm/Default/1.0.0/ubuntu/x86-64/vmlinuz"
        );
        assert_eq!(
            resolved.initramfs_storage_path(),
            "osm/Default/1.0.0/ubuntu/x86-64/initrd.img"
        );
        assert_eq!(
            resolved.install_template_storage_path(),
            "osm/Default/1.0.0/ubuntu/x86-64/autoinstall.yaml.hbs"
        );
    }
}
