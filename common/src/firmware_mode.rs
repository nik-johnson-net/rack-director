use std::fmt;

use serde::{Deserialize, Serialize};

/// Firmware mode of a device's boot environment.
///
/// Represents whether the device boots using legacy BIOS or modern UEFI firmware.
/// This affects disk partitioning requirements:
/// - BIOS+GPT requires a 2MiB `bios_grub` partition
/// - UEFI+GPT requires an ESP partition (vfat, `esp` flag)
///
/// `Option<FirmwareMode>` is used throughout the system:
/// - `None` on a device means firmware mode not yet detected, or not applicable (non-x86)
/// - `None` on a platform or role means no firmware constraint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FirmwareMode {
    Bios,
    Uefi,
}

impl fmt::Display for FirmwareMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FirmwareMode::Bios => write!(f, "BIOS"),
            FirmwareMode::Uefi => write!(f, "UEFI"),
        }
    }
}

impl FirmwareMode {
    /// Returns the lowercase string representation used for database storage.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            FirmwareMode::Bios => "bios",
            FirmwareMode::Uefi => "uefi",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_bios_serializes_to_lowercase() {
        let json = serde_json::to_string(&FirmwareMode::Bios).unwrap();
        assert_eq!(json, "\"bios\"");
    }

    #[test]
    fn test_uefi_serializes_to_lowercase() {
        let json = serde_json::to_string(&FirmwareMode::Uefi).unwrap();
        assert_eq!(json, "\"uefi\"");
    }

    #[test]
    fn test_bios_deserializes_from_lowercase() {
        let mode: FirmwareMode = serde_json::from_str("\"bios\"").unwrap();
        assert_eq!(mode, FirmwareMode::Bios);
    }

    #[test]
    fn test_uefi_deserializes_from_lowercase() {
        let mode: FirmwareMode = serde_json::from_str("\"uefi\"").unwrap();
        assert_eq!(mode, FirmwareMode::Uefi);
    }

    #[test]
    fn test_roundtrip_bios() {
        let original = FirmwareMode::Bios;
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: FirmwareMode = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, original);
    }

    #[test]
    fn test_roundtrip_uefi() {
        let original = FirmwareMode::Uefi;
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: FirmwareMode = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, original);
    }

    #[test]
    fn test_option_firmware_mode_none_serializes_to_null() {
        let mode: Option<FirmwareMode> = None;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "null");
    }

    #[test]
    fn test_option_firmware_mode_some_bios_serializes() {
        let mode: Option<FirmwareMode> = Some(FirmwareMode::Bios);
        let json = serde_json::to_value(&mode).unwrap();
        assert_eq!(json, json!("bios"));
    }

    #[test]
    fn test_option_firmware_mode_some_uefi_serializes() {
        let mode: Option<FirmwareMode> = Some(FirmwareMode::Uefi);
        let json = serde_json::to_value(&mode).unwrap();
        assert_eq!(json, json!("uefi"));
    }

    #[test]
    fn test_option_firmware_mode_none_deserializes_from_null() {
        let mode: Option<FirmwareMode> = serde_json::from_str("null").unwrap();
        assert!(mode.is_none());
    }

    #[test]
    fn test_option_firmware_mode_roundtrip() {
        let cases: Vec<Option<FirmwareMode>> =
            vec![None, Some(FirmwareMode::Bios), Some(FirmwareMode::Uefi)];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let deserialized: Option<FirmwareMode> = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, case);
        }
    }

    #[test]
    fn test_invalid_firmware_mode_fails_to_deserialize() {
        let result: Result<FirmwareMode, _> = serde_json::from_str("\"invalid\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_display_bios() {
        assert_eq!(FirmwareMode::Bios.to_string(), "BIOS");
    }

    #[test]
    fn test_display_uefi() {
        assert_eq!(FirmwareMode::Uefi.to_string(), "UEFI");
    }

    #[test]
    fn test_as_db_str_bios() {
        assert_eq!(FirmwareMode::Bios.as_db_str(), "bios");
    }

    #[test]
    fn test_as_db_str_uefi() {
        assert_eq!(FirmwareMode::Uefi.as_db_str(), "uefi");
    }
}
