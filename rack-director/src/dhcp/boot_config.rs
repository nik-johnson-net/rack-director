use anyhow::Result;
use dhcproto::v4::{self, Message};
use std::net::Ipv4Addr;

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
    /// - Pending devices (in pending_devices table) are also allowed to boot
    ///
    /// # Arguments
    /// * `device_uuid` - The device UUID if the device is known (exists in devices table)
    /// * `is_pending_device` - Whether the device exists in the pending_devices table
    ///
    /// # Returns
    /// `true` if boot options should be provided, `false` otherwise
    pub fn should_provide_boot_options(
        &self,
        device_uuid: Option<&str>,
        is_pending_device: bool,
    ) -> bool {
        self.enable_autodiscover || device_uuid.is_some() || is_pending_device
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
    /// * `is_pending_device` - Whether the device exists in the pending_devices table
    ///
    /// # Returns
    /// * `Ok(Some(BootOptions))` - Boot options for allowed devices
    /// * `Ok(None)` - Boot options withheld (unknown device with autodiscover disabled)
    /// * `Err(_)` - Error generating boot options
    pub fn get_boot_options_if_allowed(
        &self,
        mode: BootMode,
        device_uuid: Option<&str>,
        is_pending_device: bool,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(device_uuid, is_pending_device) {
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
    /// * `is_pending_device` - Whether the device exists in the pending_devices table
    ///
    /// # Returns
    /// * `Ok(Some(BootOptions))` - Boot script URL for allowed devices
    /// * `Ok(None)` - Boot script withheld (unknown device with autodiscover disabled)
    /// * `Err(_)` - Error generating boot script
    pub fn get_ipxe_boot_script_if_allowed(
        &self,
        device_uuid: Option<&str>,
        is_pending_device: bool,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(device_uuid, is_pending_device) {
            Ok(Some(self.get_ipxe_boot_script()?))
        } else {
            Ok(None)
        }
    }

    /// Applies boot options to a DHCP message for first-stage PXE boot.
    ///
    /// This method adds the following DHCP options for TFTP-based boot:
    /// - Option 66: TFTP Server Name (next_server)
    /// - Option 67: Bootfile Name (filename)
    /// - siaddr field: Next server IP address
    ///
    /// # Arguments
    /// * `msg` - The DHCP message to modify
    /// * `boot_opts` - The boot options to apply
    ///
    /// # Returns
    /// * `Ok(())` - Options applied successfully
    /// * `Err(_)` - Failed to parse next_server IP address
    pub fn apply_boot_options_to_message(
        &self,
        msg: &mut Message,
        boot_opts: &BootOptions,
    ) -> Result<()> {
        // Option 66 (TFTP Server Name)
        if let Some(next_server) = &boot_opts.next_server {
            msg.opts_mut().insert(v4::DhcpOption::TFTPServerName(
                next_server.clone().into_bytes(),
            ));
        }

        // Option 67 (Bootfile Name)
        msg.opts_mut().insert(v4::DhcpOption::BootfileName(
            boot_opts.filename.clone().into_bytes(),
        ));

        // siaddr field (next server IP)
        if let Some(next_server) = &boot_opts.next_server
            && let Ok(next_ip) = next_server.parse::<Ipv4Addr>()
        {
            msg.set_siaddr(next_ip);
        }

        Ok(())
    }

    /// Applies iPXE boot script options to a DHCP message for second-stage boot.
    ///
    /// This method adds the HTTP URL for the iPXE boot script:
    /// - Option 67: Bootfile Name (HTTP URL for iPXE script)
    ///
    /// # Arguments
    /// * `msg` - The DHCP message to modify
    /// * `boot_opts` - The boot options containing the iPXE script URL
    pub fn apply_ipxe_script_to_message(&self, msg: &mut Message, boot_opts: &BootOptions) {
        // Option 67 (Bootfile Name) - HTTP URL for iPXE script
        msg.opts_mut().insert(v4::DhcpOption::BootfileName(
            boot_opts.filename.clone().into_bytes(),
        ));
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
        assert!(provider.should_provide_boot_options(Some("device-uuid-123"), false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), true);
        // With autodiscover enabled, even unknown devices (None) should get boot options
        assert!(provider.should_provide_boot_options(None, false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        assert!(provider.should_provide_boot_options(Some("device-uuid-123"), false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        // With autodiscover disabled, unknown devices (None) should NOT get boot options
        assert!(!provider.should_provide_boot_options(None, false));
    }

    // Conditional boot options tests
    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), true);
        let result = provider
            .get_boot_options_if_allowed(BootMode::BiosLegacy, None, false)
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
            .get_boot_options_if_allowed(BootMode::BiosLegacy, None, false)
            .unwrap();
        // Should return None for unknown device with autodiscover disabled
        assert!(result.is_none());
    }

    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let result = provider
            .get_boot_options_if_allowed(BootMode::UefiBoot, Some("device-uuid-456"), false)
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
        let result = provider
            .get_ipxe_boot_script_if_allowed(None, false)
            .unwrap();
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
        let result = provider
            .get_ipxe_boot_script_if_allowed(None, false)
            .unwrap();
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
            .get_ipxe_boot_script_if_allowed(Some("device-uuid-789"), false)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    // Pending device tests
    #[test]
    fn test_should_provide_boot_options_pending_device_autodiscover_disabled() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        // Pending devices should get boot options even with autodiscover disabled
        assert!(provider.should_provide_boot_options(None, true));
    }

    #[test]
    fn test_get_boot_options_if_allowed_pending_device_autodiscover_disabled() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);
        let result = provider
            .get_boot_options_if_allowed(BootMode::UefiBoot, None, true)
            .unwrap();
        // Pending device should get boot options even with autodiscover disabled
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_pending_device_autodiscover_disabled() {
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            false,
        );
        let result = provider
            .get_ipxe_boot_script_if_allowed(None, true)
            .unwrap();
        // Pending device should get boot script even with autodiscover disabled
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    // Helper method tests
    #[test]
    fn test_apply_boot_options_to_message() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);

        let boot_opts = BootOptions {
            next_server: Some("10.0.0.1".to_string()),
            filename: "undionly.kpxe".to_string(),
        };

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .apply_boot_options_to_message(&mut msg, &boot_opts)
            .unwrap();

        // Verify TFTP Server Name (Option 66)
        let tftp_server = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::TFTPServerName(name) = opt {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
            .expect("TFTP Server Name should be present");
        assert_eq!(tftp_server, "10.0.0.1");

        // Verify Bootfile Name (Option 67)
        let bootfile = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::BootfileName(name) = opt {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
            .expect("Bootfile Name should be present");
        assert_eq!(bootfile, "undionly.kpxe");

        // Verify siaddr field
        assert_eq!(msg.siaddr().to_string(), "10.0.0.1");
    }

    #[test]
    fn test_apply_boot_options_to_message_no_next_server() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string(), false);

        let boot_opts = BootOptions {
            next_server: None,
            filename: "boot.efi".to_string(),
        };

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .apply_boot_options_to_message(&mut msg, &boot_opts)
            .unwrap();

        // TFTP Server Name should not be present
        let tftp_server = msg
            .opts()
            .iter()
            .find(|(_, opt)| matches!(opt, v4::DhcpOption::TFTPServerName(_)));
        assert!(tftp_server.is_none());

        // Bootfile Name should still be present
        let bootfile = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::BootfileName(name) = opt {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
            .expect("Bootfile Name should be present");
        assert_eq!(bootfile, "boot.efi");

        // siaddr should remain unspecified
        assert_eq!(msg.siaddr(), Ipv4Addr::UNSPECIFIED);
    }

    #[test]
    fn test_apply_ipxe_script_to_message() {
        use dhcproto::v4::{Message, Opcode};

        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            false,
        );

        let boot_opts = BootOptions {
            next_server: None,
            filename: "http://10.0.0.1:3000/cnc/ipxe".to_string(),
        };

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider.apply_ipxe_script_to_message(&mut msg, &boot_opts);

        // Verify Bootfile Name (Option 67) contains HTTP URL
        let bootfile = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::BootfileName(name) = opt {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
            .expect("Bootfile Name should be present");
        assert_eq!(bootfile, "http://10.0.0.1:3000/cnc/ipxe");
    }
}
