pub mod cnc;
pub mod device_attributes;
pub mod disk_layout;
pub mod firmware_mode;
pub mod poll_action;
mod subnet;

pub use firmware_mode::FirmwareMode;
pub use subnet::Ipv4Subnet;
pub use subnet::Ipv4SubnetError;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceScan {
    pub uuid: String,
    pub attributes: Vec<DeviceAttribute>,
}

/// Returns `true` when `mac` is a well-formed `XX:XX:XX:XX:XX:XX` MAC address:
/// six colon-separated pairs of ASCII hex digits. Case-insensitive.
pub fn is_valid_mac(mac: &str) -> bool {
    let octets: Vec<&str> = mac.split(':').collect();
    octets.len() == 6
        && octets
            .iter()
            .all(|o| o.len() == 2 && o.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::is_valid_mac;

    #[test]
    fn is_valid_mac_lowercase() {
        assert!(is_valid_mac("de:ad:be:ef:00:11"));
    }

    #[test]
    fn is_valid_mac_uppercase() {
        assert!(is_valid_mac("DE:AD:BE:EF:00:11"));
    }

    #[test]
    fn is_valid_mac_wrong_octet_count() {
        assert!(!is_valid_mac("de:ad:be:ef:00"));
        assert!(!is_valid_mac("de:ad:be:ef:00:11:22"));
    }

    #[test]
    fn is_valid_mac_non_hex_characters() {
        assert!(!is_valid_mac("de:ad:be:ef:00:zz"));
    }

    #[test]
    fn is_valid_mac_wrong_octet_length() {
        assert!(!is_valid_mac("de:ad:be:ef:0:11"));
        assert!(!is_valid_mac("de:ad:be:ef:000:11"));
    }
}
