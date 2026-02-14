use anyhow::{Result, anyhow};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::client::RackDirector;

/// BMC information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcInfo {
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub ip_address_source: String,
}

/// Default IP source for BMC configuration
fn default_ip_source() -> String {
    "static".to_string()
}

/// BMC configuration structure for setting IP and credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmcConfiguration {
    #[serde(default = "default_ip_source")]
    pub ip_address_source: String, // "static" or "dhcp"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netmask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// User slot information from ipmitool user list
#[derive(Debug, Clone)]
struct IpmiUserSlot {
    slot: u8,
    name: String,
}

/// Detect available BMC LAN channels using ipmitool channel info
///
/// This function queries channels 1-16 using `ipmitool channel info` and returns
/// a list of channels that support LAN (802.3). This fixes "Channel X is not a LAN channel!"
/// errors by dynamically detecting which channels are available instead of hardcoding [1, 2, 8].
///
/// Returns:
/// - Vec<u8> of available LAN channel numbers (e.g., [1, 8])
/// - Falls back to [1, 2, 8] if detection fails or ipmitool is unavailable
async fn detect_bmc_lan_channels() -> Vec<u8> {
    debug!("Detecting available BMC LAN channels");
    let mut lan_channels = Vec::new();

    // Query channels 1-16 (0 is reserved, most systems use 1-11)
    for channel in 1..=16 {
        match tokio::process::Command::new("ipmitool")
            .args(["channel", "info", &channel.to_string()])
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if is_lan_channel(&stdout) {
                    debug!("Found LAN channel: {}", channel);
                    lan_channels.push(channel);
                }
            }
            Ok(output) => {
                debug!(
                    "ipmitool channel info {} failed: {}",
                    channel,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                if channel == 1 {
                    // Only log on first attempt to avoid spam
                    debug!("ipmitool not available or failed to execute: {}", e);
                }
                break; // Stop trying if ipmitool not available
            }
        }
    }

    if lan_channels.is_empty() {
        warn!("No LAN channels detected, falling back to default channels [1, 2, 8]");
        vec![1, 2, 8]
    } else {
        info!(
            "Detected {} LAN channel(s): {:?}",
            lan_channels.len(),
            lan_channels
        );
        lan_channels
    }
}

/// Parse ipmitool channel info output to determine if channel is a LAN channel
///
/// Looks for "Channel Medium Type" containing "802.3 LAN" to identify LAN channels.
///
/// Example output:
/// ```
/// Channel 0x1 info:
///   Channel Medium Type   : 802.3 LAN
///   Channel Protocol Type : IPMB-1.0
///   Session Support       : multi-session
///   Active Session Count  : 0
///   Protocol Vendor ID    : 7154
/// ```
fn is_lan_channel(output: &str) -> bool {
    for line in output.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Channel Medium Type") {
            // Extract value after colon
            if let Some(colon_pos) = value.find(':') {
                let medium_type = value[colon_pos + 1..].trim();
                return medium_type.contains("802.3 LAN");
            }
        }
    }
    false
}

/// Scan for BMC (Baseboard Management Controller) using ipmitool
///
/// This function attempts to detect a BMC by running `ipmitool lan print` on
/// detected LAN channels (dynamically discovered via channel info). It parses the output to extract:
/// - MAC Address
/// - IP Address (converted to None if "0.0.0.0")
/// - IP Address Source (DHCP, Static, etc.)
///
/// Returns None if:
/// - ipmitool is not available
/// - No BMC is present
/// - Parsing fails
///
/// This is a best-effort scan and failures are non-fatal.
pub async fn scan_bmc() -> Result<Option<BmcInfo>> {
    // Detect available LAN channels dynamically
    let channels = detect_bmc_lan_channels().await;

    // Try each detected LAN channel in order
    for (idx, channel) in channels.iter().enumerate() {
        debug!("Attempting to scan BMC on channel {}", channel);

        match tokio::process::Command::new("ipmitool")
            .args(["lan", "print", &channel.to_string()])
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                // Parse the output
                if let Some(bmc_info) = parse_ipmitool_output(&stdout) {
                    info!(
                        "Discovered BMC on channel {}: MAC={}, IP={:?}, Source={}",
                        channel,
                        bmc_info.mac_address,
                        bmc_info.ip_address,
                        bmc_info.ip_address_source
                    );
                    return Ok(Some(bmc_info));
                }
            }
            Ok(output) => {
                debug!(
                    "ipmitool command failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                if idx == 0 {
                    // Only log on first attempt to avoid spam
                    debug!("ipmitool not available or failed to execute: {}", e);
                }
            }
        }
    }

    debug!("No BMC detected on any LAN channels");
    Ok(None)
}

/// Parse ipmitool lan print output to extract BMC information
fn parse_ipmitool_output(output: &str) -> Option<BmcInfo> {
    let mut mac_address: Option<String> = None;
    let mut ip_address: Option<String> = None;
    let mut ip_address_source: Option<String> = None;

    for line in output.lines() {
        let line = line.trim();

        if let Some(value) = line.strip_prefix("MAC Address") {
            // Extract everything after the first colon
            if let Some(colon_pos) = value.find(':') {
                let mac = value[colon_pos + 1..].trim().to_string();
                if !mac.is_empty() && mac != "00:00:00:00:00:00" {
                    mac_address = Some(mac);
                }
            }
        } else if let Some(value) = line.strip_prefix("IP Address Source") {
            if let Some(colon_pos) = value.find(':') {
                ip_address_source = Some(value[colon_pos + 1..].trim().to_string());
            }
        } else if line.starts_with("IP Address") && !line.contains("Source") {
            // Match "IP Address" but not "IP Address Source"
            if let Some(colon_pos) = line.find(':') {
                let ip = line[colon_pos + 1..].trim().to_string();
                // Treat 0.0.0.0 as "no IP" (None)
                if !ip.is_empty() && ip != "0.0.0.0" {
                    ip_address = Some(ip);
                }
            }
        }
    }

    // Only return BMC info if we found a valid MAC address
    if let (Some(mac), Some(source)) = (mac_address, ip_address_source) {
        Some(BmcInfo {
            mac_address: mac,
            ip_address,
            ip_address_source: source,
        })
    } else {
        None
    }
}

/// Configure BMC with static or DHCP IP address and credentials
///
/// This function configures the BMC by running ipmitool commands to set:
/// - IP address source (static or dhcp)
/// - For static: IP address, netmask, and default gateway
/// - IPMI user with specified username and password (with admin privileges)
///
/// Returns an error if ipmitool is not available or if configuration fails.
async fn configure_bmc(config: &BmcConfiguration, channel: u8) -> Result<()> {
    let ipsrc = config.ip_address_source.to_lowercase();

    info!(
        "Configuring BMC on channel {} with IP source: {}",
        channel, ipsrc
    );

    // Set IP address source (static or dhcp)
    run_ipmitool_command(
        channel,
        &["lan", "set", &channel.to_string(), "ipsrc", &ipsrc],
    )
    .await?;

    // Only set static IP fields if using static configuration
    if ipsrc == "static" {
        // Require static IP fields
        let ip_address = config
            .ip_address
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ip_address required for static BMC configuration"))?;
        let netmask = config
            .netmask
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("netmask required for static BMC configuration"))?;
        let gateway = config
            .gateway
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("gateway required for static BMC configuration"))?;

        info!("Configuring static IP: {}", ip_address);

        // Set IP address
        run_ipmitool_command(
            channel,
            &["lan", "set", &channel.to_string(), "ipaddr", ip_address],
        )
        .await?;

        // Set netmask
        run_ipmitool_command(
            channel,
            &["lan", "set", &channel.to_string(), "netmask", netmask],
        )
        .await?;

        // Set default gateway
        run_ipmitool_command(
            channel,
            &[
                "lan",
                "set",
                &channel.to_string(),
                "defgw",
                "ipaddr",
                gateway,
            ],
        )
        .await?;
    } else {
        info!("BMC will obtain IP automatically via DHCP");
    }

    // Configure IPMI user if username and password are provided
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        info!("Configuring IPMI user: {}", username);
        configure_ipmi_user(channel, username, password).await?;
        info!("IPMI user configuration completed");
    } else {
        info!("No username/password provided, skipping IPMI user configuration");
    }

    info!("BMC configuration completed successfully");
    Ok(())
}

/// Helper function to run ipmitool command
async fn run_ipmitool_command(channel: u8, args: &[&str]) -> Result<()> {
    debug!("Running ipmitool with args: {:?}", args);

    let output = tokio::process::Command::new("ipmitool")
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute ipmitool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "ipmitool command failed (channel {}): {}",
            channel,
            stderr
        ));
    }

    Ok(())
}

/// Parse ipmitool user list output to find user slots
///
/// Example output:
/// ```
/// ID  Name             Callin  Link Auth  IPMI Msg   Channel Priv Limit
/// 1                    true    false      false      Unknown (0x00)
/// 2   admin            true    false      true       ADMINISTRATOR
/// 3                    true    false      false      Unknown (0x00)
/// ```
fn parse_user_list(output: &str) -> Vec<IpmiUserSlot> {
    let mut slots = Vec::new();

    for line in output.lines().skip(1) {
        // Skip header line
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        // First column is the slot ID
        if let Ok(slot) = parts[0].parse::<u8>() {
            // Second column is the username (may be empty)
            // If it's "true" or "false", the name column is empty
            let name = if parts.len() > 1 && parts[1] != "true" && parts[1] != "false" {
                parts[1].to_string()
            } else {
                String::new()
            };

            slots.push(IpmiUserSlot { slot, name });
        }
    }

    slots
}

/// Find an empty IPMI user slot, or return slot 10 as fallback
async fn find_empty_user_slot(channel: u8) -> Result<u8> {
    info!("Searching for empty IPMI user slot on channel {}", channel);

    let output = tokio::process::Command::new("ipmitool")
        .args(["user", "list", &channel.to_string()])
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute ipmitool user list: {}", e))?;

    if !output.status.success() {
        warn!("ipmitool user list failed, using fallback slot 10");
        return Ok(10);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let slots = parse_user_list(&stdout);

    // Look for empty slot (name is empty)
    // Supermicro Hack: Skip Slot 1, which is empty but unassignable.
    for slot_info in &slots {
        if slot_info.name.is_empty() && slot_info.slot != 1 {
            info!("Found empty user slot: {}", slot_info.slot);
            return Ok(slot_info.slot);
        }
    }

    // No empty slot found, use slot 10 as fallback
    info!("No empty slots found, using fallback slot 10");
    Ok(10)
}

/// Check if a user with given name already exists
///
/// Returns the slot number if found, None otherwise
async fn find_existing_user(channel: u8, username: &str) -> Result<Option<u8>> {
    debug!(
        "Checking for existing user '{}' on channel {}",
        username, channel
    );

    let output = tokio::process::Command::new("ipmitool")
        .args(["user", "list", &channel.to_string()])
        .output()
        .await
        .map_err(|e| anyhow!("Failed to execute ipmitool user list: {}", e))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let slots = parse_user_list(&stdout);

    for slot_info in slots {
        if slot_info.name == username {
            info!(
                "Found existing user '{}' in slot {}",
                username, slot_info.slot
            );
            return Ok(Some(slot_info.slot));
        }
    }

    debug!("User '{}' not found", username);
    Ok(None)
}

/// Create or update IPMI user with credentials and admin privileges
async fn configure_ipmi_user(channel: u8, username: &str, password: &str) -> Result<()> {
    info!("Configuring IPMI user '{}'", username);

    // Check if user already exists
    let slot = match find_existing_user(channel, username).await? {
        Some(existing_slot) => {
            info!("Updating existing user in slot {}", existing_slot);
            existing_slot
        }
        None => {
            // Find empty slot
            let empty_slot = find_empty_user_slot(channel).await?;
            info!("Creating new user in slot {}", empty_slot);

            // Set username
            run_ipmitool_command(
                channel,
                &["user", "set", "name", &empty_slot.to_string(), username],
            )
            .await?;

            empty_slot
        }
    };

    // Set password
    info!("Setting password for user in slot {}", slot);
    run_ipmitool_command(
        channel,
        &["user", "set", "password", &slot.to_string(), password],
    )
    .await?;

    // Enable the user
    info!("Enabling user in slot {}", slot);
    run_ipmitool_command(channel, &["user", "enable", &slot.to_string()]).await?;

    // Set admin privileges (privilege level 4)
    info!("Setting admin privileges for slot {}", slot);
    run_ipmitool_command(
        channel,
        &["user", "priv", &slot.to_string(), "4", &channel.to_string()],
    )
    .await?;

    // Set channel access
    info!("Setting channel access for slot {}", slot);
    run_ipmitool_command(
        channel,
        &[
            "channel",
            "setaccess",
            &channel.to_string(),
            &slot.to_string(),
            "callin=on",
            "ipmi=on",
            "link=on",
            "privilege=4",
        ],
    )
    .await?;

    info!("Successfully configured IPMI user '{}'", username);
    Ok(())
}

/// Configure BMC for the current device
///
/// This action:
/// 1. Gets the device UUID from SMBIOS
/// 2. Fetches BMC configuration from rack-director
/// 3. Applies the configuration using ipmitool (tries detected LAN channels)
/// 4. Reports success or failure to rack-director
pub async fn bmc_configure(client: &RackDirector) -> Result<()> {
    info!("Starting BMC configuration...");

    // Get device UUID
    let hardware_info = crate::scan::read_dmi_for_uuid().await?;
    let uuid =
        hardware_info.ok_or_else(|| anyhow!("Failed to determine device UUID from SMBIOS"))?;

    info!("Device UUID: {}", uuid);

    // Fetch BMC configuration from rack-director
    info!("Fetching BMC configuration from rack-director...");
    let bmc_config = match client.get_bmc_config(&uuid).await {
        Ok(Some(config)) => config,
        Ok(None) => {
            // No BMC configuration is set for this device - not an error condition
            info!("No BMC configuration found for device, skipping BMC configuration");
            client.action_success(&uuid).await?;
            return Ok(());
        }
        Err(e) => {
            let error_msg = format!("Failed to fetch BMC configuration: {}", e);
            log::error!("{}", error_msg);
            client.action_failed(&uuid, &error_msg).await?;
            return Err(e);
        }
    };

    info!(
        "Retrieved BMC configuration: IP source={}",
        bmc_config.ip_address_source
    );
    if let Some(ip) = &bmc_config.ip_address {
        info!("  IP address: {}", ip);
    }

    // Convert client BmcConfig to local BmcConfiguration
    let config = BmcConfiguration {
        ip_address_source: bmc_config.ip_address_source,
        ip_address: bmc_config.ip_address,
        netmask: bmc_config.netmask,
        gateway: bmc_config.gateway,
        username: bmc_config.username,
        password: bmc_config.password,
    };

    // Detect available LAN channels and try configuring BMC on each
    let channels = detect_bmc_lan_channels().await;
    let mut last_error = None;

    for channel in channels {
        info!("Attempting to configure BMC on channel {}", channel);
        match configure_bmc(&config, channel).await {
            Ok(()) => {
                info!("BMC configured successfully on channel {}", channel);
                client.action_success(&uuid).await?;
                return Ok(());
            }
            Err(e) => {
                warn!("Failed to configure BMC on channel {}: {}", channel, e);
                last_error = Some(e);
            }
        }
    }

    // All channels failed
    let error_msg = format!(
        "Failed to configure BMC on all channels: {}",
        last_error.unwrap()
    );
    log::error!("{}", error_msg);
    client.action_failed(&uuid, &error_msg).await?;
    Err(anyhow!(error_msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for BMC configuration

    /// Test BmcConfiguration with static IP configuration
    #[test]
    fn test_bmc_configuration_static() {
        let config = BmcConfiguration {
            ip_address_source: "static".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            netmask: Some("255.255.255.0".to_string()),
            gateway: Some("192.168.1.1".to_string()),
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
        };

        assert_eq!(config.ip_address_source, "static");
        assert_eq!(config.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(config.netmask, Some("255.255.255.0".to_string()));
        assert_eq!(config.gateway, Some("192.168.1.1".to_string()));
    }

    /// Test BmcConfiguration with DHCP configuration
    #[test]
    fn test_bmc_configuration_dhcp() {
        let config = BmcConfiguration {
            ip_address_source: "dhcp".to_string(),
            ip_address: None,
            netmask: None,
            gateway: None,
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
        };

        assert_eq!(config.ip_address_source, "dhcp");
        assert_eq!(config.ip_address, None);
        assert_eq!(config.netmask, None);
        assert_eq!(config.gateway, None);
    }

    /// Test BmcConfiguration serialization
    #[test]
    fn test_bmc_configuration_serialization() {
        let config = BmcConfiguration {
            ip_address_source: "static".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            netmask: Some("255.255.255.0".to_string()),
            gateway: Some("192.168.1.1".to_string()),
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("static"));
        assert!(json.contains("192.168.1.100"));
        assert!(json.contains("255.255.255.0"));
        assert!(json.contains("192.168.1.1"));

        let deserialized: BmcConfiguration = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ip_address_source, "static");
        assert_eq!(deserialized.ip_address, Some("192.168.1.100".to_string()));
    }

    // Tests for BMC detection

    /// Test parsing valid ipmitool output
    #[test]
    fn test_parse_ipmitool_output_valid() {
        let output = r#"
Set in Progress         : Set Complete
Auth Type Support       : NONE MD2 MD5 PASSWORD
IP Address Source       : DHCP Address
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
MAC Address             : 0c:c4:7a:02:11:fe
SNMP Community String   : public
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(bmc.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(bmc.ip_address_source, "DHCP Address");
    }

    /// Test parsing ipmitool output with 0.0.0.0 IP (should be treated as None)
    #[test]
    fn test_parse_ipmitool_output_zero_ip() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : DHCP Address
IP Address              : 0.0.0.0
Subnet Mask             : 0.0.0.0
MAC Address             : 0c:c4:7a:02:11:fe
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(bmc.ip_address, None);
        assert_eq!(bmc.ip_address_source, "DHCP Address");
    }

    /// Test parsing ipmitool output with Static IP
    #[test]
    fn test_parse_ipmitool_output_static_ip() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : Static Address
IP Address              : 10.0.0.50
Subnet Mask             : 255.255.255.0
MAC Address             : aa:bb:cc:dd:ee:ff
Default Gateway IP      : 10.0.0.1
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_some());

        let bmc = result.unwrap();
        assert_eq!(bmc.mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(bmc.ip_address, Some("10.0.0.50".to_string()));
        assert_eq!(bmc.ip_address_source, "Static Address");
    }

    /// Test parsing ipmitool output missing MAC (should return None)
    #[test]
    fn test_parse_ipmitool_output_missing_mac() {
        let output = r#"
Set in Progress         : Set Complete
IP Address Source       : DHCP Address
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test parsing ipmitool output missing IP source (should return None)
    #[test]
    fn test_parse_ipmitool_output_missing_ip_source() {
        let output = r#"
Set in Progress         : Set Complete
IP Address              : 192.168.1.100
Subnet Mask             : 255.255.255.0
MAC Address             : 0c:c4:7a:02:11:fe
"#;

        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test parsing empty ipmitool output
    #[test]
    fn test_parse_ipmitool_output_empty() {
        let output = "";
        let result = parse_ipmitool_output(output);
        assert!(result.is_none());
    }

    /// Test BmcInfo serialization
    #[test]
    fn test_bmc_info_serialization() {
        let bmc = BmcInfo {
            mac_address: "0c:c4:7a:02:11:fe".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            ip_address_source: "DHCP Address".to_string(),
        };

        let json = serde_json::to_string(&bmc).unwrap();
        assert!(json.contains("0c:c4:7a:02:11:fe"));
        assert!(json.contains("192.168.1.100"));
        assert!(json.contains("DHCP Address"));

        let deserialized: BmcInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.mac_address, "0c:c4:7a:02:11:fe");
        assert_eq!(deserialized.ip_address, Some("192.168.1.100".to_string()));
        assert_eq!(deserialized.ip_address_source, "DHCP Address");
    }

    /// Test BmcInfo with no IP address
    #[test]
    fn test_bmc_info_no_ip() {
        let bmc = BmcInfo {
            mac_address: "0c:c4:7a:02:11:fe".to_string(),
            ip_address: None,
            ip_address_source: "DHCP Address".to_string(),
        };

        let json = serde_json::to_string(&bmc).unwrap();
        assert!(json.contains("0c:c4:7a:02:11:fe"));
        assert!(json.contains("null"));
        assert!(json.contains("DHCP Address"));

        let deserialized: BmcInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ip_address, None);
    }

    // Tests for IPMI user management

    /// Test parsing ipmitool user list output
    #[test]
    fn test_parse_user_list_valid() {
        let output = r#"ID  Name             Callin  Link Auth  IPMI Msg   Channel Priv Limit
1                    true    false      false      Unknown (0x00)
2   admin            true    false      true       ADMINISTRATOR
3                    true    false      false      Unknown (0x00)
4   RACKDIRECTOR     true    false      true       ADMINISTRATOR
10                   true    false      false      Unknown (0x00)"#;

        let slots = parse_user_list(output);

        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].slot, 1);
        assert_eq!(slots[0].name, "");
        assert_eq!(slots[1].slot, 2);
        assert_eq!(slots[1].name, "admin");
        assert_eq!(slots[3].slot, 4);
        assert_eq!(slots[3].name, "RACKDIRECTOR");
        assert_eq!(slots[4].slot, 10);
        assert_eq!(slots[4].name, "");
    }

    /// Test parsing empty user list
    #[test]
    fn test_parse_user_list_empty() {
        let output = "ID  Name             Callin  Link Auth  IPMI Msg   Channel Priv Limit\n";
        let slots = parse_user_list(output);
        assert_eq!(slots.len(), 0);
    }

    /// Test parsing malformed user list (graceful handling)
    #[test]
    fn test_parse_user_list_malformed() {
        let output = r#"ID  Name
invalid line
not a number  username
3   testuser"#;

        let slots = parse_user_list(output);

        // Should only parse valid lines
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].slot, 3);
        assert_eq!(slots[0].name, "testuser");
    }

    /// Test parsing user list with extra whitespace
    #[test]
    fn test_parse_user_list_whitespace() {
        let output = r#"ID  Name             Callin  Link Auth  IPMI Msg   Channel Priv Limit
1                    true    false      false      Unknown (0x00)
2   admin            true    false      true       ADMINISTRATOR"#;

        let slots = parse_user_list(output);

        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].slot, 1);
        assert_eq!(slots[0].name, "");
        assert_eq!(slots[1].slot, 2);
        assert_eq!(slots[1].name, "admin");
    }

    // Tests for channel detection

    /// Test parsing ipmitool channel info output for LAN channel
    #[test]
    fn test_is_lan_channel_valid() {
        let output = r#"
Channel 0x1 info:
  Channel Medium Type   : 802.3 LAN
  Channel Protocol Type : IPMB-1.0
  Session Support       : multi-session
  Active Session Count  : 0
  Protocol Vendor ID    : 7154
"#;
        assert!(is_lan_channel(output));
    }

    /// Test parsing ipmitool channel info output for non-LAN channel
    #[test]
    fn test_is_lan_channel_serial() {
        let output = r#"
Channel 0x0 info:
  Channel Medium Type   : System Interface
  Channel Protocol Type : KCS
  Session Support       : session-less
  Active Session Count  : 0
"#;
        assert!(!is_lan_channel(output));
    }

    /// Test parsing ipmitool channel info output for system management bus
    #[test]
    fn test_is_lan_channel_smbus() {
        let output = r#"
Channel 0x6 info:
  Channel Medium Type   : System Management Bus (SMBus)
  Channel Protocol Type : IPMB-1.0
  Session Support       : session-less
  Active Session Count  : 0
"#;
        assert!(!is_lan_channel(output));
    }

    /// Test parsing empty channel info output
    #[test]
    fn test_is_lan_channel_empty() {
        let output = "";
        assert!(!is_lan_channel(output));
    }
}
