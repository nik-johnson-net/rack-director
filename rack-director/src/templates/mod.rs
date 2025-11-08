use crate::operating_systems::OperatingSystem;
use crate::roles::Role;
use anyhow::Result;
use handlebars::Handlebars;
use serde_json::json;

/// Network information for a device
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub mac_address: String,
    pub ip_address: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub netmask: String,
}

/// Device attributes for template rendering
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub uuid: String,
    pub hostname: Option<String>,
}

/// Render an install script template with device-specific variables
///
/// Available template variables:
/// - {{ device.uuid }} - Device UUID
/// - {{ device.hostname }} - Device hostname
/// - {{ device.mac_address }} - Primary MAC address
/// - {{ device.ip_address }} - IP address (static or DHCP lease)
/// - {{ device.gateway }} - Network gateway
/// - {{ device.dns_servers }} - DNS servers (space-separated)
/// - {{ device.netmask }} - Network netmask
/// - {{ role.name }} - Role name
/// - {{ role.disk_layout }} - Disk layout as JSON
/// - {{ os.name }} - OS name
/// - {{ os.version }} - OS version
/// - {{ config.* }} - Any custom config from role.config_template
pub fn render_install_script(
    template: &str,
    device: &DeviceInfo,
    role: &Role,
    os: &OperatingSystem,
    network: &NetworkInfo,
) -> Result<String> {
    let mut handlebars = Handlebars::new();

    // Don't HTML-escape output (we're generating config files, not HTML)
    handlebars.register_escape_fn(handlebars::no_escape);

    // Build context with all available variables
    let context = json!({
        "device": {
            "uuid": device.uuid,
            "hostname": device.hostname.as_deref().unwrap_or("unknown"),
            "mac_address": network.mac_address,
            "ip_address": network.ip_address,
            "gateway": network.gateway,
            "dns_servers": network.dns_servers.join(" "),
            "netmask": network.netmask,
        },
        "role": {
            "name": role.name,
            "disk_layout": role.disk_layout,
        },
        "os": {
            "name": os.name,
            "version": os.version,
        },
        "config": role.config_template,
    });

    Ok(handlebars.render_template(template, &context)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::DiskLayout;

    #[test]
    fn test_render_simple_template() {
        let template = "hostname: {{ device.hostname }}";
        let device = DeviceInfo {
            uuid: "test-uuid".to_string(),
            hostname: Some("server01".to_string()),
        };
        let role = Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout: DiskLayout { partitions: vec![] },
            config_template: None,
            created_at: None,
            updated_at: None,
        };
        let os = OperatingSystem {
            id: Some(1),
            name: "Ubuntu".to_string(),
            version: "24.04".to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        };
        let network = NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            netmask: "255.255.255.0".to_string(),
        };

        let result = render_install_script(template, &device, &role, &os, &network).unwrap();
        assert_eq!(result, "hostname: server01");
    }

    #[test]
    fn test_render_network_template() {
        let template = r#"
network:
  address: {{ device.ip_address }}
  gateway: {{ device.gateway }}
  netmask: {{ device.netmask }}
  dns: {{ device.dns_servers }}
"#;
        let device = DeviceInfo {
            uuid: "test-uuid".to_string(),
            hostname: Some("server01".to_string()),
        };
        let role = Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout: DiskLayout { partitions: vec![] },
            config_template: None,
            created_at: None,
            updated_at: None,
        };
        let os = OperatingSystem {
            id: Some(1),
            name: "Ubuntu".to_string(),
            version: "24.04".to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        };
        let network = NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            netmask: "255.255.255.0".to_string(),
        };

        let result = render_install_script(template, &device, &role, &os, &network).unwrap();
        assert!(result.contains("address: 10.0.0.100"));
        assert!(result.contains("gateway: 10.0.0.1"));
        assert!(result.contains("dns: 8.8.8.8 8.8.4.4"));
    }

    #[test]
    fn test_render_with_custom_config() {
        let template = "{{#each config.packages}}{{ this }} {{/each}}";
        let device = DeviceInfo {
            uuid: "test-uuid".to_string(),
            hostname: Some("server01".to_string()),
        };
        let role = Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout: DiskLayout { partitions: vec![] },
            config_template: Some(json!({
                "packages": ["nginx", "postgresql", "redis"]
            })),
            created_at: None,
            updated_at: None,
        };
        let os = OperatingSystem {
            id: Some(1),
            name: "Ubuntu".to_string(),
            version: "24.04".to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        };
        let network = NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec![],
            netmask: "255.255.255.0".to_string(),
        };

        let result = render_install_script(template, &device, &role, &os, &network).unwrap();
        assert_eq!(result, "nginx postgresql redis ");
    }

    #[test]
    fn test_render_debian_preseed() {
        let template = r#"
d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}
"#;
        let device = DeviceInfo {
            uuid: "test-uuid".to_string(),
            hostname: Some("debian-server".to_string()),
        };
        let role = Role {
            id: Some(1),
            name: "test-role".to_string(),
            description: None,
            os_id: 1,
            disk_layout: DiskLayout { partitions: vec![] },
            config_template: None,
            created_at: None,
            updated_at: None,
        };
        let os = OperatingSystem {
            id: Some(1),
            name: "Debian".to_string(),
            version: "12".to_string(),
            description: None,
            created_at: None,
            updated_at: None,
        };
        let network = NetworkInfo {
            mac_address: "00:11:22:33:44:55".to_string(),
            ip_address: "10.0.0.100".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            netmask: "255.255.255.0".to_string(),
        };

        let result = render_install_script(template, &device, &role, &os, &network).unwrap();
        assert!(result.contains("d-i netcfg/get_hostname string debian-server"));
        assert!(result.contains("d-i netcfg/get_ipaddress string 10.0.0.100"));
    }
}
