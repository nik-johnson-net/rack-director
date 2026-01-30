use anyhow::Result;
use dhcproto::v4::{self, DhcpOption, Message, OptionCode};
use std::net::Ipv4Addr;
use uuid::Uuid;

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
}

impl BootConfigProvider {
    /// Creates a new BootConfigProvider.
    ///
    /// # Arguments
    /// * `tftp_server` - TFTP server address (IP or hostname)
    /// * `http_server` - HTTP server URL (must start with `http://` or `https://`)
    ///
    /// # Panics
    /// Panics if `http_server` does not start with `http://` or `https://`.
    /// This validation ensures boot scripts always receive valid HTTP URLs.
    pub fn new(tftp_server: String, http_server: String) -> Self {
        // Validate that http_server has proper URL scheme
        if !http_server.starts_with("http://") && !http_server.starts_with("https://") {
            panic!(
                "http_server must start with 'http://' or 'https://', got: '{}'",
                http_server
            );
        }

        Self {
            tftp_server,
            http_server,
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

    /// Checks if the client requested boot options via DHCP option 55 (Parameter Request List).
    ///
    /// This method examines the Parameter Request List (option 55) in the DHCP message to
    /// determine if the client explicitly requested boot-related options:
    /// - Option 66 (TFTP Server Name)
    /// - Option 67 (Bootfile Name)
    ///
    /// # Arguments
    /// * `msg` - The DHCP message to check
    ///
    /// # Returns
    /// * `true` - Client requested both boot options (66 and 67)
    /// * `false` - Client did not request boot options or option 55 is missing
    ///
    /// # Use Case
    /// This is used in conjunction with per-network autodiscovery to determine if a device
    /// is actively trying to PXE boot. Devices that don't request boot options (e.g., already
    /// booted operating systems renewing leases) should not receive boot options.
    pub fn client_requested_boot_options(&self, msg: &Message) -> bool {
        if let Some(param_list) = msg.opts().iter().find_map(|(_, opt)| {
            if let DhcpOption::ParameterRequestList(list) = opt {
                Some(list)
            } else {
                None
            }
        }) {
            param_list.contains(&OptionCode::TFTPServerName)
                && param_list.contains(&OptionCode::BootfileName)
        } else {
            false
        }
    }

    /// Determines whether boot options should be provided for a device.
    ///
    /// # Decision Logic
    /// - If network autodiscover is enabled: Always provide boot options (permissive mode)
    /// - If network autodiscover is disabled: Only provide boot options for known devices (strict mode)
    /// - Pending devices (in pending_devices table) are also allowed to boot
    ///
    /// # Arguments
    /// * `network_autodiscover` - Whether autodiscovery is enabled for this network
    /// * `device_uuid` - The device UUID if the device is known (exists in devices table)
    /// * `is_pending_device` - Whether the device exists in the pending_devices table
    ///
    /// # Returns
    /// `true` if boot options should be provided, `false` otherwise
    pub fn should_provide_boot_options(
        &self,
        network_autodiscover: bool,
        device_uuid: Option<&Uuid>,
        is_pending_device: bool,
    ) -> bool {
        network_autodiscover || device_uuid.is_some() || is_pending_device
    }

    /// Gets boot options for the specified mode if allowed based on device state.
    ///
    /// This method combines device authorization checking with boot option retrieval.
    /// It returns `None` if the device is not allowed to boot (unknown device with
    /// autodiscover disabled), or `Some(BootOptions)` if allowed.
    ///
    /// # Arguments
    /// * `mode` - The boot mode (BIOS Legacy, UEFI, etc.)
    /// * `network_autodiscover` - Whether autodiscovery is enabled for this network
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
        network_autodiscover: bool,
        device_uuid: Option<&Uuid>,
        is_pending_device: bool,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(network_autodiscover, device_uuid, is_pending_device) {
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
    /// * `network_autodiscover` - Whether autodiscovery is enabled for this network
    /// * `device_uuid` - The device UUID if known
    /// * `is_pending_device` - Whether the device exists in the pending_devices table
    ///
    /// # Returns
    /// * `Ok(Some(BootOptions))` - Boot script URL for allowed devices
    /// * `Ok(None)` - Boot script withheld (unknown device with autodiscover disabled)
    /// * `Err(_)` - Error generating boot script
    pub fn get_ipxe_boot_script_if_allowed(
        &self,
        network_autodiscover: bool,
        device_uuid: Option<&Uuid>,
        is_pending_device: bool,
    ) -> Result<Option<BootOptions>> {
        if self.should_provide_boot_options(network_autodiscover, device_uuid, is_pending_device) {
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
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    #[test]
    fn test_bios_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::BiosLegacy).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "undionly.kpxe");
    }

    #[test]
    fn test_uefi_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::UefiBoot).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_uefi_arm64_boot_options() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::UefiArm64).unwrap();
        assert_eq!(opts.next_server, Some("10.0.0.1".to_owned()));
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_ipxe_boot_script() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let opts = provider.get_ipxe_boot_script().unwrap();
        assert_eq!(opts.next_server, None);
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    // Autodiscover decision logic tests
    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        assert!(provider.should_provide_boot_options(true, Some(&test_uuid()), false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        // With autodiscover enabled, even unknown devices (None) should get boot options
        assert!(provider.should_provide_boot_options(true, None, false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        assert!(provider.should_provide_boot_options(false, Some(&test_uuid()), false));
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        // With autodiscover disabled, unknown devices (None) should NOT get boot options
        assert!(!provider.should_provide_boot_options(false, None, false));
    }

    // Conditional boot options tests
    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let result = provider
            .get_boot_options_if_allowed(BootMode::BiosLegacy, true, None, false)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "undionly.kpxe");
    }

    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let result = provider
            .get_boot_options_if_allowed(BootMode::BiosLegacy, false, None, false)
            .unwrap();
        // Should return None for unknown device with autodiscover disabled
        assert!(result.is_none());
    }

    #[test]
    fn test_get_boot_options_if_allowed_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let result = provider
            .get_boot_options_if_allowed(BootMode::UefiBoot, false, Some(&test_uuid()), false)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "ipxe.efi");
    }

    // Conditional iPXE boot script tests
    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_enabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let result = provider
            .get_ipxe_boot_script_if_allowed(true, None, false)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_disabled_unknown_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let result = provider
            .get_ipxe_boot_script_if_allowed(false, None, false)
            .unwrap();
        // Should return None for unknown device with autodiscover disabled
        assert!(result.is_none());
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_autodiscover_disabled_known_device() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let result = provider
            .get_ipxe_boot_script_if_allowed(false, Some(&test_uuid()), false)
            .unwrap();
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    // Pending device tests
    #[test]
    fn test_should_provide_boot_options_pending_device_autodiscover_disabled() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        // Pending devices should get boot options even with autodiscover disabled
        assert!(provider.should_provide_boot_options(false, None, true));
    }

    #[test]
    fn test_get_boot_options_if_allowed_pending_device_autodiscover_disabled() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        let result = provider
            .get_boot_options_if_allowed(BootMode::UefiBoot, false, None, true)
            .unwrap();
        // Pending device should get boot options even with autodiscover disabled
        assert!(result.is_some());
        let opts = result.unwrap();
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_get_ipxe_boot_script_if_allowed_pending_device_autodiscover_disabled() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let result = provider
            .get_ipxe_boot_script_if_allowed(false, None, true)
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
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

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
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

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

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());

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

    // Tests for client_requested_boot_options
    #[test]
    fn test_client_requested_boot_options_with_both_options() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // Add Parameter Request List with options 66 and 67
        msg.opts_mut()
            .insert(v4::DhcpOption::ParameterRequestList(vec![
                OptionCode::SubnetMask,
                OptionCode::Router,
                OptionCode::TFTPServerName, // Option 66
                OptionCode::BootfileName,   // Option 67
                OptionCode::DomainNameServer,
            ]));

        assert!(
            provider.client_requested_boot_options(&msg),
            "Should return true when both options 66 and 67 are requested"
        );
    }

    #[test]
    fn test_client_requested_boot_options_missing_tftp_server() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // Add Parameter Request List with only option 67
        msg.opts_mut()
            .insert(v4::DhcpOption::ParameterRequestList(vec![
                OptionCode::SubnetMask,
                OptionCode::Router,
                OptionCode::BootfileName, // Option 67 only
                OptionCode::DomainNameServer,
            ]));

        assert!(
            !provider.client_requested_boot_options(&msg),
            "Should return false when option 66 is missing"
        );
    }

    #[test]
    fn test_client_requested_boot_options_missing_bootfile() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // Add Parameter Request List with only option 66
        msg.opts_mut()
            .insert(v4::DhcpOption::ParameterRequestList(vec![
                OptionCode::SubnetMask,
                OptionCode::Router,
                OptionCode::TFTPServerName, // Option 66 only
                OptionCode::DomainNameServer,
            ]));

        assert!(
            !provider.client_requested_boot_options(&msg),
            "Should return false when option 67 is missing"
        );
    }

    #[test]
    fn test_client_requested_boot_options_no_param_list() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // No Parameter Request List at all
        assert!(
            !provider.client_requested_boot_options(&msg),
            "Should return false when Parameter Request List is missing"
        );
    }

    #[test]
    fn test_client_requested_boot_options_empty_param_list() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // Empty Parameter Request List
        msg.opts_mut()
            .insert(v4::DhcpOption::ParameterRequestList(vec![]));

        assert!(
            !provider.client_requested_boot_options(&msg),
            "Should return false when Parameter Request List is empty"
        );
    }

    #[test]
    fn test_client_requested_boot_options_typical_os_renewal() {
        use dhcproto::v4::{Message, Opcode};

        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);

        // Typical parameter request list from an already-booted OS
        msg.opts_mut()
            .insert(v4::DhcpOption::ParameterRequestList(vec![
                OptionCode::SubnetMask,
                OptionCode::Router,
                OptionCode::DomainNameServer,
                OptionCode::DomainName,
                OptionCode::AddressLeaseTime,
            ]));

        assert!(
            !provider.client_requested_boot_options(&msg),
            "Should return false for typical OS DHCP renewal (no boot options requested)"
        );
    }

    // URL validation tests
    #[test]
    fn test_new_with_valid_http_url() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1".to_string());
        assert_eq!(provider.http_server, "http://10.0.0.1");
    }

    #[test]
    fn test_new_with_valid_https_url() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "https://10.0.0.1".to_string());
        assert_eq!(provider.http_server, "https://10.0.0.1");
    }

    #[test]
    fn test_new_with_valid_http_url_with_port() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        assert_eq!(provider.http_server, "http://10.0.0.1:3000");
    }

    #[test]
    #[should_panic(expected = "http_server must start with 'http://' or 'https://'")]
    fn test_new_with_invalid_url_no_scheme() {
        BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string());
    }

    #[test]
    #[should_panic(expected = "http_server must start with 'http://' or 'https://'")]
    fn test_new_with_invalid_url_wrong_scheme() {
        BootConfigProvider::new("10.0.0.1".to_string(), "ftp://10.0.0.1".to_string());
    }

    #[test]
    fn test_ipxe_boot_script_url_has_http_prefix() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "http://10.0.0.1:3000".to_string());
        let opts = provider.get_ipxe_boot_script().unwrap();

        // Verify the URL starts with http://
        assert!(
            opts.filename.starts_with("http://"),
            "iPXE boot script URL should start with http://, got: {}",
            opts.filename
        );
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }

    #[test]
    fn test_ipxe_boot_script_url_preserves_https() {
        let provider =
            BootConfigProvider::new("10.0.0.1".to_string(), "https://10.0.0.1".to_string());
        let opts = provider.get_ipxe_boot_script().unwrap();

        // Verify the URL starts with https://
        assert!(
            opts.filename.starts_with("https://"),
            "iPXE boot script URL should start with https://, got: {}",
            opts.filename
        );
        assert_eq!(opts.filename, "https://10.0.0.1/cnc/ipxe");
    }
}
