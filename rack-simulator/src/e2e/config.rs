use anyhow::{Context, Result};
use common::disk_layout::DiskLayout;
use serde::Deserialize;
use std::path::Path;

/// Top-level test configuration loaded from a TOML file.
#[derive(Debug, Deserialize)]
pub struct TestConfig {
    pub test: TestMeta,
    pub vm: VmConfig,
    pub rack_director: RackDirectorSetup,
    pub lifecycle: LifecycleConfig,
}

/// Test metadata.
#[derive(Debug, Deserialize)]
pub struct TestMeta {
    pub name: String,
    pub description: Option<String>,
    pub timeout_seconds: u64,
}

/// VM resource configuration.
#[derive(Debug, Deserialize)]
pub struct VmConfig {
    pub memory_mb: u32,
    pub disks: Vec<DiskSpec>,
}

/// Specification for a single virtual disk.
#[derive(Debug, Deserialize)]
pub struct DiskSpec {
    pub size_gb: u64,
}

/// rack-director setup: platforms and roles to create before the test.
#[derive(Debug, Deserialize)]
pub struct RackDirectorSetup {
    pub platforms: Vec<PlatformSpec>,
    pub roles: Vec<RoleSpec>,
}

/// Platform specification.
#[derive(Debug, Deserialize)]
pub struct PlatformSpec {
    pub name: String,
    pub attributes: serde_json::Value,
}

/// Role specification.
#[derive(Debug, Deserialize)]
pub struct RoleSpec {
    pub name: String,
    pub platform: Option<String>,
    pub disk_layout: DiskLayout,
}

/// Lifecycle configuration: steps to drive and expected final state.
#[derive(Debug, Deserialize)]
pub struct LifecycleConfig {
    pub steps: Vec<LifecycleStep>,
    pub expect_final_state: String,
}

/// A single lifecycle transition.
#[derive(Debug, Deserialize)]
pub struct LifecycleStep {
    pub from: String,
    pub to: String,
}

impl TestConfig {
    /// Load a test configuration from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read test config: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse test config: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_valid_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
[test]
name = "test-basic"
timeout_seconds = 300

[vm]
memory_mb = 512
disks = [{{ size_gb = 20 }}]

[[rack_director.platforms]]
name = "test-platform"
[rack_director.platforms.attributes]
total_memory_mb = 512

[[rack_director.roles]]
name = "test-role"
platform = "test-platform"

[rack_director.roles.disk_layout]
disks = []

[[lifecycle.steps]]
from = "new"
to = "unprovisioned"

[lifecycle]
expect_final_state = "unprovisioned"
"#
        )
        .unwrap();

        let config = TestConfig::load(f.path()).unwrap();
        assert_eq!(config.test.name, "test-basic");
        assert_eq!(config.test.timeout_seconds, 300);
        assert_eq!(config.vm.memory_mb, 512);
        assert_eq!(config.vm.disks.len(), 1);
        assert_eq!(config.vm.disks[0].size_gb, 20);
        assert_eq!(config.rack_director.platforms.len(), 1);
        assert_eq!(config.rack_director.roles.len(), 1);
        assert_eq!(config.lifecycle.steps.len(), 1);
        assert_eq!(config.lifecycle.expect_final_state, "unprovisioned");
    }

    #[test]
    fn test_load_missing_file() {
        let result = TestConfig::load(Path::new("/nonexistent/path/test.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "this is not valid toml !!!{{}}").unwrap();
        let result = TestConfig::load(f.path());
        assert!(result.is_err());
    }
}
