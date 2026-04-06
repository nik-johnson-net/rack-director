use serde::{Deserialize, Serialize};

/// Configuration for an operating system defined in an OSM archive.
///
/// Corresponds to an `<os-slug>.toml` file inside the archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperatingSystemConfig {
    pub name: String,
    pub release: String,
    pub architectures: Vec<ArchitectureConfig>,
    #[serde(default)]
    pub template_variables: Vec<TemplateVariable>,
}

/// Architecture-specific configuration for an operating system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchitectureConfig {
    pub arch: String,
    pub kernel: String,
    pub initramfs: String,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub cmdline: String,
    pub install_template: String,
}

/// A user-configurable variable exposed by an OS install template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateVariable {
    pub name: String,
    #[serde(rename = "type")]
    pub var_type: TemplateVariableType,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<toml::Value>,
}

/// The data type of a template variable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TemplateVariableType {
    String,
    List,
    Boolean,
    Integer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
name = "Ubuntu"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
modules = ["squashfs", "overlay"]
cmdline = "quiet splash"
install_template = "ubuntu-2204.sh"

[[template_variables]]
name = "root_password"
type = "string"
description = "Root user password"
required = true
"#;
        let config: OperatingSystemConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "Ubuntu");
        assert_eq!(config.release, "22.04");
        assert_eq!(config.architectures.len(), 1);
        assert_eq!(config.architectures[0].arch, "x86-64");
        assert_eq!(config.architectures[0].modules, vec!["squashfs", "overlay"]);
        assert_eq!(config.architectures[0].cmdline, "quiet splash");
        assert_eq!(config.template_variables.len(), 1);
        assert_eq!(config.template_variables[0].name, "root_password");
        assert!(config.template_variables[0].required);
    }

    #[test]
    fn test_deserialize_minimal_config() {
        let toml_str = r#"
name = "Minimal OS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let config: OperatingSystemConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "Minimal OS");
        assert!(config.architectures[0].modules.is_empty());
        assert_eq!(config.architectures[0].cmdline, "");
        assert!(config.template_variables.is_empty());
    }

    #[test]
    fn test_deserialize_multiple_architectures() {
        let toml_str = r#"
name = "Multi-Arch OS"
release = "2.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz-x64"
initramfs = "initrd-x64.img"
install_template = "install-x64.sh"

[[architectures]]
arch = "aarch64"
kernel = "vmlinuz-arm64"
initramfs = "initrd-arm64.img"
install_template = "install-arm64.sh"
"#;
        let config: OperatingSystemConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.architectures.len(), 2);
        assert_eq!(config.architectures[0].arch, "x86-64");
        assert_eq!(config.architectures[1].arch, "aarch64");
    }

    #[test]
    fn test_deserialize_missing_name_fails() {
        let toml_str = r#"
release = "22.04"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"
"#;
        let result: Result<OperatingSystemConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_template_variable_types() {
        let toml_str = r#"
name = "Test OS"
release = "1.0"

[[architectures]]
arch = "x86-64"
kernel = "vmlinuz"
initramfs = "initrd.img"
install_template = "install.sh"

[[template_variables]]
name = "hostname"
type = "string"
description = "System hostname"

[[template_variables]]
name = "dns_servers"
type = "list"
description = "DNS server list"

[[template_variables]]
name = "enable_ssh"
type = "boolean"
description = "Enable SSH daemon"

[[template_variables]]
name = "max_connections"
type = "integer"
description = "Max connections"
"#;
        let config: OperatingSystemConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.template_variables.len(), 4);
        assert_eq!(
            config.template_variables[0].var_type,
            TemplateVariableType::String
        );
        assert_eq!(
            config.template_variables[1].var_type,
            TemplateVariableType::List
        );
        assert_eq!(
            config.template_variables[2].var_type,
            TemplateVariableType::Boolean
        );
        assert_eq!(
            config.template_variables[3].var_type,
            TemplateVariableType::Integer
        );
    }

    #[test]
    fn test_roundtrip_serialize_deserialize() {
        let config = OperatingSystemConfig {
            name: "Test OS".to_string(),
            release: "3.0".to_string(),
            architectures: vec![ArchitectureConfig {
                arch: "x86-64".to_string(),
                kernel: "vmlinuz".to_string(),
                initramfs: "initrd.img".to_string(),
                modules: vec!["ext4".to_string()],
                cmdline: "console=ttyS0".to_string(),
                install_template: "install.sh".to_string(),
            }],
            template_variables: vec![TemplateVariable {
                name: "disk".to_string(),
                var_type: TemplateVariableType::String,
                description: "Target disk".to_string(),
                required: true,
                default: None,
            }],
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: OperatingSystemConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}
