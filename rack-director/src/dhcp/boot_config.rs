use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    BiosLegacy,
    UefiBoot,
    UefiArm64,
}

#[derive(Debug, Clone)]
pub struct BootOptions {
    pub next_server: Option<String>,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct BootConfigProvider {
    tftp_server: String,
    http_server: String,
    enable_autodiscover: bool,
}

impl BootConfigProvider {
    pub fn new(tftp_server: String, http_server: String, enable_autodiscover: bool) -> Self {
        Self {
            tftp_server,
            http_server,
            enable_autodiscover,
        }
    }

    pub fn get_boot_options(&self, mode: BootMode) -> Result<BootOptions> {
        match mode {
            BootMode::BiosLegacy => Ok(BootOptions {
                // BIOS: Use TFTP to download undionly.kpxe (iPXE for BIOS)
                // Then iPXE will fetch the boot script via HTTP
                next_server: Some(self.tftp_server.clone()),
                filename: "undionly.kpxe".to_string(),
            }),
            BootMode::UefiBoot => Ok(BootOptions {
                // UEFI x86-64: Use TFTP to download ipxe.efi (iPXE for UEFI)
                // Then iPXE will fetch the boot script via HTTP
                next_server: Some(self.tftp_server.clone()),
                filename: "ipxe.efi".to_string(),
            }),
            BootMode::UefiArm64 => Ok(BootOptions {
                // UEFI ARM64: Use TFTP to download ipxe.efi (iPXE for UEFI ARM64)
                // Then iPXE will fetch the boot script via HTTP
                next_server: Some(self.tftp_server.clone()),
                filename: "ipxe.efi".to_string(),
            }),
        }
    }

    pub fn get_ipxe_boot_script(&self) -> Result<BootOptions> {
        // iPXE second-stage: Return HTTP URL for boot script
        // This is called when iPXE makes a second DHCP request after booting
        Ok(BootOptions {
            next_server: None,
            filename: format!("{}/cnc/ipxe", self.http_server),
        })
    }

    /// Determines whether boot options should be provided for a device.
    ///
    /// # Decision Logic
    /// - If autodiscover is enabled: Always provide boot options (permissive mode)
    /// - If autodiscover is disabled: Only provide boot options for known devices (strict mode)
    ///
    /// # Arguments
    /// * `device_uuid` - The device UUID if the device is known (exists in devices table)
    ///
    /// # Returns
    /// `true` if boot options should be provided, `false` otherwise
    pub fn should_provide_boot_options(&self, device_uuid: Option<&str>) -> bool {
        self.enable_autodiscover || device_uuid.is_some()
    }

    /// Gets boot options for the specified mode if allowed based on device state.
    ///
    /// This method combines device authorization checking with boot option retrieval.
    /// It returns `None` if the device is not allowed to boot (unknown device with
    /// autodiscover disabled), or `Some(BootOptions)` if allowed.
    ///
    /// # Arguments
    /// * `mode` - The boot mode (BIOS Legacy, UEFI, etc.)
    /// * `device_uuid` - The device UUID if known
    ///
    /// # Returns
    /// * `Ok(Some(BootOptions))` - Boot options for allowed devices
    /// * `Ok(None)` - Boot options withheld (unknown device with autodiscover disabled)
    /// * `Err(_)` - Error generating boot options
    pub fn get_boot_options_if_allowed(
        &self,
        mode: BootMode,
        device_uuid: Option<&str>,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(device_uuid) {
            Ok(Some(self.get_boot_options(mode)?))
        } else {
            Ok(None)
        }
    }

    /// Gets iPXE boot script if allowed based on device state.
    ///
    /// This method combines device authorization checking with boot script retrieval.
    /// It returns `None` if the device is not allowed to boot (unknown device with
    /// autodiscover disabled), or `Some(BootOptions)` if allowed.
    ///
    /// # Arguments
    /// * `device_uuid` - The device UUID if known
    ///
    /// # Returns
    /// * `Ok(Some(BootOptions))` - Boot script URL for allowed devices
    /// * `Ok(None)` - Boot script withheld (unknown device with autodiscover disabled)
    /// * `Err(_)` - Error generating boot script
    pub fn get_ipxe_boot_script_if_allowed(
        &self,
        device_uuid: Option<&str>,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(device_uuid) {
            Ok(Some(self.get_ipxe_boot_script()?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bios_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let opts = provider.get_boot_options(BootMode::BiosLegacy).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "undionly.kpxe");
    }

    #[test]
    fn test_uefi_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let opts = provider.get_boot_options(BootMode::UefiBoot).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_uefi_arm64_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let opts = provider.get_boot_options(BootMode::UefiArm64).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_ipxe_boot_script() {
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            false,
        );
        let opts = provider.get_ipxe_boot_script().unwrap();
        assert_eq!(opts.next_server, None);
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    // Autodiscover decision logic tests
    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), true);
        assert!(provider.should_provide_boot_options(Some("device-uuid-123")));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), true);
        // With autodiscover enabled, even unknown devices (None) should get boot options
        assert!(provider.should_provide_boot_options(None));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        assert!(provider.should_provide_boot_options(Some("device-uuid-123")));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        // With autodiscover disabled, unknown devices (None) should NOT get boot options
        assert!(!provider.should_provide_boot_options(None));
    }

    // Conditional boot options tests
    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), true);
        let result = provider
            .get_boot_options_if_allowed(BootMode::BiosLegacy, None)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "undionly.kpxe");
    }

    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let result = provider
            .get_boot_options_if_allowed(BootMode::BiosLegacy, None)
            .unwrap();
        // Should return None for unknown device with autodiscover disabled
        assert!(result.is_none());
    }

    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let result = provider
            .get_boot_options_if_allowed(BootMode::UefiBoot, Some("device-uuid-456"))
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "ipxe.efi");
    }

    // Conditional iPXE boot script tests
    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_enabled_unknown_device() {
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            true,
        );
        let result = provider.get_ipxe_boot_script_if_allowed(None).unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_disabled_unknown_device() {
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            false,
        );
        let result = provider.get_ipxe_boot_script_if_allowed(None).unwrap();
        // Should return None for unknown device with autodiscover disabled
        assert!(result.is_none());
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_disabled_known_device() {
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            false,
        );
        let result = provider
            .get_ipxe_boot_script_if_allowed(Some("device-uuid-789"))
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }
}
