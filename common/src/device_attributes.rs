use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::str::FromStr;

/// Disk type classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiskType {
    Nvme,
    Ssd,
    Hdd,
}

impl FromStr for DiskType {
    type Err = String;

    /// Parse disk type from string (case-insensitive)
    /// Returns an error for unknown disk type strings
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "nvme" => Ok(DiskType::Nvme),
            "ssd" => Ok(DiskType::Ssd),
            "hdd" => Ok(DiskType::Hdd),
            _ => Err(format!("Unknown disk type: {}", s)),
        }
    }
}

impl DiskType {
    /// Get priority for disk selection (lower is better/faster)
    pub fn priority(self) -> u8 {
        match self {
            DiskType::Nvme => 1,
            DiskType::Ssd => 2,
            DiskType::Hdd => 3,
        }
    }
}

/// Top-level device attributes structure
///
/// This struct provides type-safe access to device hardware and configuration attributes.
/// It uses `#[serde(flatten)]` on the `extra` field to maintain backward compatibility
/// with existing JSON data that may contain unknown fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceAttributes {
    /// Hostname assigned to the device
    #[serde(default)]
    pub hostname: Option<String>,

    /// System manufacturer (e.g., "Dell Inc.", "Supermicro")
    #[serde(default)]
    pub manufacturer: Option<String>,

    /// Product name/model (e.g., "PowerEdge R640")
    #[serde(default)]
    pub product_name: Option<String>,

    /// System serial number
    #[serde(default)]
    pub serial_number: Option<String>,

    /// BIOS version string
    #[serde(default)]
    pub bios_version: Option<String>,

    /// BIOS vendor name
    #[serde(default)]
    pub bios_vendor: Option<String>,

    /// Network interfaces detected on the device
    #[serde(default)]
    pub network_interfaces: Vec<NetworkInterface>,

    /// Baseboard Management Controller (BMC) information
    #[serde(default)]
    pub bmc: Option<BmcInfo>,

    /// BMC configuration (IP settings, credentials)
    #[serde(default)]
    pub bmc_config: Option<BmcConfig>,

    /// Disk drives detected on the device
    #[serde(default)]
    pub disks: Vec<DiskInfo>,

    /// CPUs detected on the device
    #[serde(default)]
    pub cpus: Vec<CpuInfo>,

    /// Memory modules detected on the device
    #[serde(default)]
    pub memory: Vec<MemoryInfo>,

    /// Device-specific kernel command line arguments
    #[serde(default)]
    pub cmdline_args: Option<String>,

    /// Device-related warnings and status messages
    /// Contains informational messages about device state, including:
    /// - Platform detection status messages
    /// - Hardware configuration warnings
    /// - Other device-specific alerts
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,

    /// Legacy field - MAC address (prefer network_interfaces)
    #[serde(default)]
    pub mac_address: Option<String>,

    /// Legacy field - static IP (prefer network_interfaces)
    #[serde(default)]
    pub static_ip: Option<String>,

    /// Catch-all for unknown/custom fields
    /// This ensures backward compatibility with existing JSON data
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Network interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Interface name (e.g., "eth0", "ens0")
    pub interface_name: String,

    /// MAC address in standard format (e.g., "aa:bb:cc:dd:ee:ff")
    pub mac_address: String,

    /// Assigned IP address, if any
    #[serde(default)]
    pub ip_address: Option<String>,

    /// Network ID this interface is on (if it has an IP)
    #[serde(default)]
    pub network_id: Option<i64>,

    /// Interface speed in Mbps (megabits per second), if available
    /// May be None if link is down or speed cannot be determined
    #[serde(default)]
    pub speed_mbps: Option<u32>,

    /// Whether this interface is disabled (e.g., due to duplicate MAC)
    #[serde(default)]
    pub disabled: bool,

    /// Warning message explaining why interface is disabled
    #[serde(default)]
    pub warning_label: Option<String>,
}

/// Baseboard Management Controller (BMC) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcInfo {
    /// BMC MAC address
    pub mac_address: String,

    /// BMC IP address, if discovered
    #[serde(default)]
    pub ip_address: Option<String>,

    /// How the IP was assigned (e.g., "DHCP", "Static", "Unknown")
    #[serde(default)]
    pub ip_address_source: Option<String>,
}

/// BMC configuration to be applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcConfig {
    /// IP address source ("dhcp" or "static")
    pub ip_address_source: String,

    /// Static IP address (required if ip_address_source is "static")
    /// Serializes as string in JSON (e.g., "192.168.1.100")
    #[serde(default)]
    pub ip_address: Option<Ipv4Addr>,

    /// Netmask (required if ip_address_source is "static")
    /// Serializes as string in JSON (e.g., "255.255.255.0")
    #[serde(default)]
    pub netmask: Option<Ipv4Addr>,

    /// Gateway (required if ip_address_source is "static")
    /// Serializes as string in JSON (e.g., "192.168.1.1")
    #[serde(default)]
    pub gateway: Option<Ipv4Addr>,

    /// BMC admin username
    #[serde(default)]
    pub username: Option<String>,

    /// BMC admin password
    #[serde(default)]
    pub password: Option<String>,
}

/// Disk drive information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Disk device name (e.g., "sda", "nvme0n1")
    pub name: String,

    /// Disk size in gigabytes (GB)
    /// Converted from hardware scan (512-byte blocks) to GB for consistency
    #[serde(default)]
    pub size: Option<u64>,

    /// Disk type (nvme, ssd, or hdd)
    #[serde(default)]
    pub disk_type: Option<DiskType>,

    /// Disk model name
    #[serde(default)]
    pub model: Option<String>,

    /// Disk serial number
    /// Read from /sys/block/{name}/device/serial
    #[serde(default)]
    pub serial: Option<String>,

    /// Disk vendor name
    /// Read from /sys/block/{name}/device/vendor
    #[serde(default)]
    pub vendor: Option<String>,

    /// World Wide Name (WWN) identifier for the disk
    /// Obtained from a wwn-* symlink in /dev/disk/by-id/ or via udevadm ID_WWN.
    /// Provides a globally unique, stable identity across reboots and slot changes.
    #[serde(default)]
    pub uuid: Option<String>,

    /// Stable bus-based device path (e.g., "/dev/disk/by-path/pci-0000:00:1f.2-ata-1")
    /// This path persists across reboots and is used for platform matching.
    /// Prefer this over simple device names like "/dev/sda" which can change.
    #[serde(default)]
    pub path: Option<String>,
}

/// CPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    /// CPU socket designation (e.g., "CPU 1", "CPU 2")
    #[serde(default)]
    pub designation: Option<String>,

    /// CPU manufacturer (e.g., "Intel", "AMD")
    #[serde(default)]
    pub manufacturer: Option<String>,

    #[serde(default)]
    pub model: Option<String>,

    /// Number of physical cores
    #[serde(default)]
    pub cores: Option<u32>,

    /// Number of logical threads
    #[serde(default)]
    pub threads: Option<u32>,

    /// Clock speed in MHz
    #[serde(default)]
    pub speed_mhz: Option<u32>,
}

/// Memory module information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    /// Memory module size in megabytes (MB)
    #[serde(default)]
    pub size_mb: Option<u16>,

    /// Memory speed in MHz
    #[serde(default)]
    pub speed_mhz: Option<u32>,

    /// Memory manufacturer
    #[serde(default)]
    pub manufacturer: Option<String>,

    /// Memory part number
    #[serde(default)]
    pub part_number: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_device_attributes_default() {
        let attrs = DeviceAttributes::default();
        assert!(attrs.hostname.is_none());
        assert!(attrs.network_interfaces.is_empty());
        assert!(attrs.extra.is_empty());
        assert!(attrs.warnings.is_empty());
    }

    #[test]
    fn test_device_attributes_serialization_roundtrip() {
        let attrs = DeviceAttributes {
            hostname: Some("test-server".to_string()),
            manufacturer: Some("Dell Inc.".to_string()),
            product_name: Some("PowerEdge R640".to_string()),
            network_interfaces: vec![NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(1),
                speed_mbps: Some(10000),
                disabled: false,
                warning_label: None,
            }],
            ..Default::default()
        };

        // Serialize to JSON
        let json_str = serde_json::to_string(&attrs).unwrap();

        // Deserialize back
        let deserialized: DeviceAttributes = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.hostname, Some("test-server".to_string()));
        assert_eq!(deserialized.manufacturer, Some("Dell Inc.".to_string()));
        assert_eq!(deserialized.network_interfaces.len(), 1);
        assert_eq!(
            deserialized.network_interfaces[0].mac_address,
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(deserialized.network_interfaces[0].speed_mbps, Some(10000));
    }

    #[test]
    fn test_backward_compatibility_old_json() {
        // Simulate old JSON format without typed fields
        let old_json = json!({
            "hostname": "legacy-server",
            "mac_address": "aa:bb:cc:dd:ee:ff",
            "static_ip": "10.0.0.50"
        });

        let attrs: DeviceAttributes = serde_json::from_value(old_json).unwrap();

        assert_eq!(attrs.hostname, Some("legacy-server".to_string()));
        assert_eq!(attrs.mac_address, Some("aa:bb:cc:dd:ee:ff".to_string()));
        assert_eq!(attrs.static_ip, Some("10.0.0.50".to_string()));
        assert!(attrs.network_interfaces.is_empty());
    }

    #[test]
    fn test_flatten_captures_unknown_fields() {
        // JSON with unknown custom fields
        let json = json!({
            "hostname": "test-server",
            "custom_field_1": "value1",
            "custom_field_2": 42,
            "nested_custom": {
                "subfield": "data"
            }
        });

        let attrs: DeviceAttributes = serde_json::from_value(json).unwrap();

        assert_eq!(attrs.hostname, Some("test-server".to_string()));

        // Unknown fields should be in extra
        assert_eq!(
            attrs.extra.get("custom_field_1").unwrap().as_str().unwrap(),
            "value1"
        );
        assert_eq!(
            attrs.extra.get("custom_field_2").unwrap().as_u64().unwrap(),
            42
        );
        assert!(attrs.extra.get("nested_custom").unwrap().is_object());
    }

    #[test]
    fn test_flatten_preserves_unknown_fields_on_serialization() {
        let mut attrs = DeviceAttributes {
            hostname: Some("test".to_string()),
            ..Default::default()
        };

        // Add custom fields to extra
        attrs.extra.insert(
            "custom_key".to_string(),
            serde_json::Value::String("custom_value".to_string()),
        );

        // Serialize
        let json_value = serde_json::to_value(&attrs).unwrap();

        // Verify the custom field appears at top level (not nested)
        assert_eq!(
            json_value.get("custom_key").unwrap().as_str().unwrap(),
            "custom_value"
        );
        assert_eq!(
            json_value.get("hostname").unwrap().as_str().unwrap(),
            "test"
        );
    }

    #[test]
    fn test_network_interface_defaults() {
        let json = json!({
            "interface_name": "eth0",
            "mac_address": "aa:bb:cc:dd:ee:ff"
        });

        let iface: NetworkInterface = serde_json::from_value(json).unwrap();

        assert_eq!(iface.interface_name, "eth0");
        assert_eq!(iface.mac_address, "aa:bb:cc:dd:ee:ff");
        assert!(iface.ip_address.is_none());
        assert!(iface.network_id.is_none());
        assert!(iface.speed_mbps.is_none());
        assert!(!iface.disabled);
        assert!(iface.warning_label.is_none());
    }

    #[test]
    fn test_bmc_info_serialization() {
        let bmc = BmcInfo {
            mac_address: "11:22:33:44:55:66".to_string(),
            ip_address: Some("10.0.1.10".to_string()),
            ip_address_source: Some("DHCP".to_string()),
        };

        let json_str = serde_json::to_string(&bmc).unwrap();
        let deserialized: BmcInfo = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.mac_address, "11:22:33:44:55:66");
        assert_eq!(deserialized.ip_address, Some("10.0.1.10".to_string()));
        assert_eq!(deserialized.ip_address_source, Some("DHCP".to_string()));
    }

    #[test]
    fn test_disk_info_serialization() {
        let disk = DiskInfo {
            name: "sda".to_string(),
            size: Some(480),
            disk_type: Some(DiskType::Ssd),
            model: Some("Samsung 860 EVO".to_string()),
            serial: Some("S3Z9NX0M123456".to_string()),
            vendor: None,
            uuid: None,
            path: Some("/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string()),
        };

        let json_str = serde_json::to_string(&disk).unwrap();
        let deserialized: DiskInfo = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.name, "sda");
        assert_eq!(deserialized.size, Some(480));
        assert_eq!(deserialized.disk_type, Some(DiskType::Ssd));
    }

    #[test]
    fn test_cpu_info_defaults() {
        let json = json!({});
        let cpu: CpuInfo = serde_json::from_value(json).unwrap();

        assert!(cpu.designation.is_none());
        assert!(cpu.manufacturer.is_none());
        assert!(cpu.cores.is_none());
        assert!(cpu.threads.is_none());
        assert!(cpu.speed_mhz.is_none());
    }

    #[test]
    fn test_memory_info_defaults() {
        let json = json!({});
        let mem: MemoryInfo = serde_json::from_value(json).unwrap();

        assert!(mem.size_mb.is_none());
        assert!(mem.speed_mhz.is_none());
        assert!(mem.manufacturer.is_none());
        assert!(mem.part_number.is_none());
    }

    #[test]
    fn test_merge_partial_updates() {
        // Simulate existing device attributes
        let existing = DeviceAttributes {
            hostname: Some("server-01".to_string()),
            manufacturer: Some("Dell".to_string()),
            ..Default::default()
        };

        // Serialize to JSON
        let mut existing_json = serde_json::to_value(&existing).unwrap();

        // Simulate partial update from agent (adding new fields)
        let update = serde_json::json!({
            "product_name": "PowerEdge R640",
            "serial_number": "ABC123"
        });

        // Merge update into existing
        let existing_map = existing_json.as_object_mut().unwrap();
        for (key, value) in update.as_object().unwrap() {
            existing_map.insert(key.clone(), value.clone());
        }

        // Deserialize back
        let merged: DeviceAttributes = serde_json::from_value(existing_json).unwrap();

        // Verify old fields preserved
        assert_eq!(merged.hostname, Some("server-01".to_string()));
        assert_eq!(merged.manufacturer, Some("Dell".to_string()));

        // Verify new fields added
        assert_eq!(merged.product_name, Some("PowerEdge R640".to_string()));
        assert_eq!(merged.serial_number, Some("ABC123".to_string()));
    }

    #[test]
    fn test_partial_update_overwrites_existing() {
        // Simulate existing device attributes
        let existing = DeviceAttributes {
            hostname: Some("old-hostname".to_string()),
            manufacturer: Some("Dell".to_string()),
            ..Default::default()
        };

        // Serialize to JSON
        let mut existing_json = serde_json::to_value(&existing).unwrap();

        // Simulate partial update that overwrites hostname
        let update = serde_json::json!({
            "hostname": "new-hostname"
        });

        // Merge update into existing
        let existing_map = existing_json.as_object_mut().unwrap();
        for (key, value) in update.as_object().unwrap() {
            existing_map.insert(key.clone(), value.clone());
        }

        // Deserialize back
        let merged: DeviceAttributes = serde_json::from_value(existing_json).unwrap();

        // Verify hostname was updated
        assert_eq!(merged.hostname, Some("new-hostname".to_string()));

        // Verify other fields preserved
        assert_eq!(merged.manufacturer, Some("Dell".to_string()));
    }

    #[test]
    fn test_bmc_config_serialization_with_ipv4() {
        let bmc = BmcConfig {
            ip_address_source: "static".to_string(),
            ip_address: Some("10.0.1.100".parse().unwrap()),
            netmask: Some("255.255.255.0".parse().unwrap()),
            gateway: Some("10.0.1.1".parse().unwrap()),
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&bmc).unwrap();
        println!("BmcConfig JSON:\n{}", json);

        // Verify it uses string format
        assert!(json.contains(r#""10.0.1.100""#));
        assert!(json.contains(r#""255.255.255.0""#));
        assert!(json.contains(r#""10.0.1.1""#));

        // Deserialize back
        let deserialized: BmcConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ip_address, Some("10.0.1.100".parse().unwrap()));
        assert_eq!(deserialized.netmask, Some("255.255.255.0".parse().unwrap()));
        assert_eq!(deserialized.gateway, Some("10.0.1.1".parse().unwrap()));
    }

    #[test]
    fn test_ipv4addr_default_serialization() {
        // Test how Ipv4Addr serializes by default with serde_json
        use std::net::Ipv4Addr;

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestConfig {
            ip_address: Option<Ipv4Addr>,
            netmask: Option<Ipv4Addr>,
        }

        let config = TestConfig {
            ip_address: Some("192.168.1.100".parse().unwrap()),
            netmask: Some("255.255.255.0".parse().unwrap()),
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&config).unwrap();
        println!("Default Ipv4Addr serialization:\n{}", json);

        // Check if it serializes as string or array
        assert!(
            json.contains(r#""192.168.1.100""#)
                || json.contains("[192,168,1,100]")
                || json.contains("[192, 168, 1, 100]"),
            "JSON should contain IP address in some format"
        );

        // Try to deserialize from string format
        let json_string = r#"{
            "ip_address": "192.168.1.100",
            "netmask": "255.255.255.0"
        }"#;

        let result_from_string = serde_json::from_str::<TestConfig>(json_string);
        println!("Deserialize from string format: {:?}", result_from_string);

        // Try to deserialize from array format
        let json_array = r#"{
            "ip_address": [192, 168, 1, 100],
            "netmask": [255, 255, 255, 0]
        }"#;

        let result_from_array = serde_json::from_str::<TestConfig>(json_array);
        println!("Deserialize from array format: {:?}", result_from_array);

        // Determine which format works
        if let Ok(item) = result_from_string {
            println!("✓ String format works!");
            assert_eq!(item, config);
        } else if let Ok(array) = result_from_array {
            println!("✓ Array format works!");
            assert_eq!(array, config);
        } else {
            panic!(
                "Neither string nor array format works! String error: {:?}, Array error: {:?}",
                result_from_string.err(),
                result_from_array.err()
            );
        }
    }

    #[test]
    fn test_complex_device_attributes() {
        let json = json!({
            "hostname": "server-01",
            "manufacturer": "Dell Inc.",
            "product_name": "PowerEdge R640",
            "serial_number": "ABC123",
            "network_interfaces": [
                {
                    "interface_name": "eth0",
                    "mac_address": "aa:bb:cc:dd:ee:01",
                    "ip_address": "10.0.0.100",
                    "network_id": 1
                },
                {
                    "interface_name": "eth1",
                    "mac_address": "aa:bb:cc:dd:ee:02",
                    "ip_address": "10.0.0.101"
                }
            ],
            "bmc": {
                "mac_address": "11:22:33:44:55:66",
                "ip_address": "10.0.1.10",
                "ip_address_source": "DHCP"
            },
            "disks": [
                {
                    "name": "sda",
                    "size": 480,
                    "disk_type": "ssd"
                }
            ],
            "cpus": [
                {
                    "designation": "CPU 1",
                    "manufacturer": "Intel",
                    "cores": 8,
                    "threads": 16,
                    "speed_mhz": 2400
                }
            ],
            "custom_field": "custom_value"
        });

        let attrs: DeviceAttributes = serde_json::from_value(json).unwrap();

        assert_eq!(attrs.hostname, Some("server-01".to_string()));
        assert_eq!(attrs.manufacturer, Some("Dell Inc.".to_string()));
        assert_eq!(attrs.network_interfaces.len(), 2);
        assert!(attrs.bmc.is_some());
        assert_eq!(attrs.disks.len(), 1);
        assert_eq!(attrs.cpus.len(), 1);
        assert_eq!(attrs.cpus[0].cores, Some(8));

        // Custom field should be captured
        assert_eq!(
            attrs.extra.get("custom_field").unwrap().as_str().unwrap(),
            "custom_value"
        );
    }

    #[test]
    fn test_disk_type_from_str() {
        assert_eq!("nvme".parse::<DiskType>().unwrap(), DiskType::Nvme);
        assert_eq!("NVME".parse::<DiskType>().unwrap(), DiskType::Nvme);
        assert_eq!("ssd".parse::<DiskType>().unwrap(), DiskType::Ssd);
        assert_eq!("SSD".parse::<DiskType>().unwrap(), DiskType::Ssd);
        assert_eq!("hdd".parse::<DiskType>().unwrap(), DiskType::Hdd);
        assert_eq!("HDD".parse::<DiskType>().unwrap(), DiskType::Hdd);
        // Unknown types return an error, but can default to HDD
        assert_eq!(
            "unknown".parse::<DiskType>().unwrap_or(DiskType::Hdd),
            DiskType::Hdd
        );
        assert_eq!(
            "invalid".parse::<DiskType>().unwrap_or(DiskType::Hdd),
            DiskType::Hdd
        );
        // Test that parse errors actually occur for invalid types
        assert!("unknown".parse::<DiskType>().is_err());
        assert!("invalid".parse::<DiskType>().is_err());
    }

    #[test]
    fn test_disk_type_priority() {
        assert!(DiskType::Nvme.priority() < DiskType::Ssd.priority());
        assert!(DiskType::Ssd.priority() < DiskType::Hdd.priority());
        assert_eq!(DiskType::Nvme.priority(), 1);
        assert_eq!(DiskType::Ssd.priority(), 2);
        assert_eq!(DiskType::Hdd.priority(), 3);
    }

    #[test]
    fn test_disk_type_serialization() {
        // Test serialization to lowercase JSON
        assert_eq!(serde_json::to_string(&DiskType::Nvme).unwrap(), "\"nvme\"");
        assert_eq!(serde_json::to_string(&DiskType::Ssd).unwrap(), "\"ssd\"");
        assert_eq!(serde_json::to_string(&DiskType::Hdd).unwrap(), "\"hdd\"");

        // Test deserialization from lowercase JSON
        assert_eq!(
            serde_json::from_str::<DiskType>("\"nvme\"").unwrap(),
            DiskType::Nvme
        );
        assert_eq!(
            serde_json::from_str::<DiskType>("\"ssd\"").unwrap(),
            DiskType::Ssd
        );
        assert_eq!(
            serde_json::from_str::<DiskType>("\"hdd\"").unwrap(),
            DiskType::Hdd
        );
    }

    #[test]
    fn test_disk_info_with_disk_type_enum() {
        let disk = DiskInfo {
            name: "nvme0n1".to_string(),
            size: Some(1000),
            disk_type: Some(DiskType::Nvme),
            model: Some("Samsung 970 EVO".to_string()),
            serial: Some("S123456".to_string()),
            vendor: None,
            uuid: None,
            path: Some("/dev/disk/by-path/pci-0000:01:00.0-nvme-1".to_string()),
        };

        // Serialize and verify JSON format
        let json = serde_json::to_value(&disk).unwrap();
        assert_eq!(json["name"], "nvme0n1");
        assert_eq!(json["size"], 1000);
        assert_eq!(json["disk_type"], "nvme");

        // Deserialize back
        let json_str = serde_json::to_string(&disk).unwrap();
        let deserialized: DiskInfo = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.disk_type, Some(DiskType::Nvme));
        assert_eq!(deserialized.size, Some(1000));
    }

    #[test]
    fn test_network_interface_with_speed() {
        let iface = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(1),
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        };

        // Serialize and verify JSON format
        let json = serde_json::to_value(&iface).unwrap();
        assert_eq!(json["interface_name"], "eth0");
        assert_eq!(json["mac_address"], "aa:bb:cc:dd:ee:ff");
        assert_eq!(json["speed_mbps"], 10000);

        // Deserialize back
        let json_str = serde_json::to_string(&iface).unwrap();
        let deserialized: NetworkInterface = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.speed_mbps, Some(10000));
    }

    #[test]
    fn test_network_interface_without_speed() {
        let iface = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: None,
            network_id: None,
            speed_mbps: None,
            disabled: false,
            warning_label: None,
        };

        // Serialize and verify JSON format
        let json = serde_json::to_value(&iface).unwrap();
        assert_eq!(json["interface_name"], "eth0");
        assert_eq!(json["mac_address"], "aa:bb:cc:dd:ee:ff");
        assert_eq!(json["speed_mbps"], serde_json::Value::Null);

        // Deserialize back
        let json_str = serde_json::to_string(&iface).unwrap();
        let deserialized: NetworkInterface = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.speed_mbps, None);
    }

    #[test]
    fn test_network_interface_backward_compatibility() {
        // Old JSON without speed_mbps field should deserialize correctly
        let json = json!({
            "interface_name": "eth0",
            "mac_address": "aa:bb:cc:dd:ee:ff",
            "ip_address": "10.0.0.100"
        });

        let iface: NetworkInterface = serde_json::from_value(json).unwrap();
        assert_eq!(iface.interface_name, "eth0");
        assert_eq!(iface.speed_mbps, None);
    }

    #[test]
    fn test_warnings_serialization() {
        let mut attrs = DeviceAttributes::default();
        attrs.warnings = vec![
            "Platform auto-detection failed: no matching platform found".to_string(),
            "High memory usage detected".to_string(),
        ];

        let json = serde_json::to_value(&attrs).unwrap();
        let warnings = json.get("warnings").unwrap().as_array().unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(
            warnings[0].as_str().unwrap(),
            "Platform auto-detection failed: no matching platform found"
        );
        assert_eq!(warnings[1].as_str().unwrap(), "High memory usage detected");

        // Deserialize back
        let deserialized: DeviceAttributes = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.warnings.len(), 2);
        assert_eq!(
            deserialized.warnings[0],
            "Platform auto-detection failed: no matching platform found"
        );
    }

    #[test]
    fn test_warnings_empty_vec_skipped_in_json() {
        let attrs = DeviceAttributes::default();
        let json = serde_json::to_value(&attrs).unwrap();

        // Empty warnings vec should not appear in JSON
        assert!(json.get("warnings").is_none());
    }

    #[test]
    fn test_warnings_backward_compatibility() {
        // Old JSON without warnings should deserialize with empty vec
        let json = json!({
            "hostname": "test-server",
            "manufacturer": "Dell Inc."
        });

        let attrs: DeviceAttributes = serde_json::from_value(json).unwrap();
        assert_eq!(attrs.hostname, Some("test-server".to_string()));
        assert!(attrs.warnings.is_empty());
    }

    #[test]
    fn test_warnings_platform_detection_failure_message() {
        // Test that the failure warning message can be stored
        let message = "Platform auto-detection failed: no matching platform found";

        let mut attrs = DeviceAttributes::default();
        attrs.warnings = vec![message.to_string()];

        // Serialize and deserialize
        let json_str = serde_json::to_string(&attrs).unwrap();
        let deserialized: DeviceAttributes = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.warnings.len(), 1);
        assert_eq!(deserialized.warnings[0], message);
    }

    #[test]
    fn test_disk_info_vendor_and_uuid_fields() {
        let disk = DiskInfo {
            name: "sda".to_string(),
            size: Some(480),
            disk_type: Some(DiskType::Ssd),
            model: Some("SAMSUNG MZWLL800HEHP".to_string()),
            serial: Some("S123456789".to_string()),
            vendor: Some("ATA".to_string()),
            uuid: Some("0x5002538d41628a5e".to_string()),
            path: Some("/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string()),
        };

        let json = serde_json::to_value(&disk).unwrap();
        assert_eq!(json["vendor"], "ATA");
        assert_eq!(json["uuid"], "0x5002538d41628a5e");

        let json_str = serde_json::to_string(&disk).unwrap();
        let deserialized: DiskInfo = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.vendor, Some("ATA".to_string()));
        assert_eq!(deserialized.uuid, Some("0x5002538d41628a5e".to_string()));
    }

    #[test]
    fn test_disk_info_vendor_uuid_none_backward_compat() {
        // Old JSON without vendor/uuid fields should deserialize with None values
        let json = json!({
            "name": "sda",
            "size": 480,
            "disk_type": "ssd",
            "serial": "S123456789",
            "path": "/dev/disk/by-path/pci-0000:00:1f.2-ata-1"
        });

        let disk: DiskInfo = serde_json::from_value(json).unwrap();
        assert_eq!(disk.name, "sda");
        assert!(disk.vendor.is_none());
        assert!(disk.uuid.is_none());
    }

    #[test]
    fn test_disk_info_vendor_uuid_null_in_json() {
        // JSON with explicit null vendor/uuid should also deserialize correctly
        let json = json!({
            "name": "nvme0n1",
            "size": 1000,
            "disk_type": "nvme",
            "vendor": null,
            "uuid": null,
            "path": "/dev/disk/by-path/pci-0000:01:00.0-nvme-1"
        });

        let disk: DiskInfo = serde_json::from_value(json).unwrap();
        assert_eq!(disk.name, "nvme0n1");
        assert!(disk.vendor.is_none());
        assert!(disk.uuid.is_none());
    }
}
