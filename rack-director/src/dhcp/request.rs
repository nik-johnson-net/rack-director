use dhcproto::v4::{Architecture, DhcpOption, Message, MessageType, OptionCode};
use std::net::Ipv4Addr;
use uuid::Uuid;

use super::store::format_mac;

/// Extract Server Identifier (Option 54) from a DHCP message.
///
/// Per RFC 2131, the Server Identifier option is used by clients to identify
/// which DHCP server sent an OFFER, and by servers to determine if a REQUEST
/// is intended for them.
///
/// # Arguments
/// * `msg` - The DHCP message to extract the server identifier from
///
/// # Returns
/// * `Some(Ipv4Addr)` - The server identifier if present and valid
/// * `None` - If the option is missing or malformed
///
/// # RFC 2131 Context
/// - In SELECTING state: Client includes Server Identifier from chosen OFFER
/// - In INIT-REBOOT state: No Server Identifier (client verifying old IP)
/// - In RENEWING/REBINDING state: No Server Identifier (lease renewal)
pub fn extract_server_identifier(msg: &Message) -> Option<Ipv4Addr> {
    msg.opts()
        .get(OptionCode::ServerIdentifier)
        .and_then(|opt| {
            if let DhcpOption::ServerIdentifier(ip) = opt {
                Some(*ip)
            } else {
                None
            }
        })
}

/// Pre-parsed DHCP request options extracted in a single pass.
pub struct RequestContext {
    pub mac: String,
    #[allow(dead_code)]
    pub message_type: MessageType,
    pub requested_ip: Option<Ipv4Addr>,
    pub client_arch: Option<Architecture>,
    pub is_ipxe: bool,
    pub requested_tftp_server: bool,
    pub requested_bootfile: bool,
    pub requested_bootfile_size: bool,
    pub ciaddr: Ipv4Addr,
    pub guid: Option<Uuid>,
}

impl RequestContext {
    /// Extract all relevant options from a DHCP message in a single pass.
    pub fn from_message(msg: &Message) -> Self {
        let mac = format_mac(msg.chaddr());
        let mut message_type = MessageType::Discover; // safe default
        let mut requested_ip = None;
        let mut client_arch = None;
        let mut is_ipxe = false;
        let mut has_tftp_server_name = false;
        let mut has_bootfile_name = false;
        let mut has_bootfile_size = false;

        for (_code, opt) in msg.opts().iter() {
            match opt {
                DhcpOption::MessageType(mt) => message_type = *mt,
                DhcpOption::RequestedIpAddress(ip) => requested_ip = Some(*ip),
                DhcpOption::ClientSystemArchitecture(arch) => client_arch = Some(*arch),
                DhcpOption::UserClass(data) if data == b"iPXE" => is_ipxe = true,
                DhcpOption::ParameterRequestList(list) => {
                    has_tftp_server_name = list.contains(&OptionCode::TFTPServerName);
                    has_bootfile_name = list.contains(&OptionCode::BootfileName);
                    has_bootfile_size = list.contains(&OptionCode::BootFileSize);
                }
                _ => {}
            }
        }

        let guid = extract_guid(msg);

        Self {
            mac,
            message_type,
            requested_ip,
            client_arch,
            is_ipxe,
            requested_tftp_server: has_tftp_server_name,
            requested_bootfile: has_bootfile_name,
            requested_bootfile_size: has_bootfile_size,
            ciaddr: msg.ciaddr(),
            guid,
        }
    }
}

/// Extract GUID from DHCP Option 97 (Client Machine Identifier).
///
/// Per RFC 4578, Option 97 contains:
/// - 1 byte type field (0 = GUID/UUID)
/// - 16 bytes UUID data
///
/// SMBIOS UUIDs use mixed-endian byte order:
/// - First 3 groups (time_low, time_mid, time_hi_and_version): little-endian
/// - Last 2 groups (clock_seq, node): big-endian
///
/// We use `Uuid::from_bytes_le()` which handles this correctly.
fn extract_guid(msg: &Message) -> Option<Uuid> {
    // DHCP Option 97 - Client Machine Identifier
    // dhcproto may expose this as Unknown(97) with raw bytes
    for (code, opt) in msg.opts().iter() {
        if code == &OptionCode::Unknown(97)
            && let DhcpOption::Unknown(unknown_opt) = opt
        {
            // UnknownOption has a data() method that returns &[u8]
            let data = unknown_opt.data();
            // First byte is type (0 = GUID), remaining 16 bytes are UUID
            if data.len() == 17
                && data[0] == 0
                && let Ok(uuid_bytes) = data[1..17].try_into()
            {
                return Some(Uuid::from_bytes_le(uuid_bytes));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dhcproto::v4::Opcode;

    #[test]
    fn test_extract_guid_with_valid_option() {
        use dhcproto::v4::UnknownOption;

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        // Create Option 97 with type 0 and a valid UUID
        // UUID format: 550e8400-e29b-41d4-a716-446655440000
        // In little-endian bytes for first 3 groups
        let mut option_data = vec![0u8]; // Type byte = 0
        option_data.extend_from_slice(&[
            0x00, 0x84, 0x0e, 0x55, // time_low (little-endian)
            0x9b, 0xe2, // time_mid (little-endian)
            0xd4, 0x41, // time_hi_and_version (little-endian)
            0xa7, 0x16, // clock_seq (big-endian)
            0x44, 0x66, 0x55, 0x44, 0x00, 0x00, // node (big-endian)
        ]);

        msg.opts_mut()
            .insert(DhcpOption::Unknown(UnknownOption::new(
                OptionCode::Unknown(97),
                option_data.clone(),
            )));

        let guid = extract_guid(&msg);
        assert!(guid.is_some(), "GUID should be extracted from valid option");

        let expected = Uuid::from_bytes_le([
            0x00, 0x84, 0x0e, 0x55, 0x9b, 0xe2, 0xd4, 0x41, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]);
        assert_eq!(guid.unwrap(), expected);
    }

    #[test]
    fn test_extract_guid_missing_option() {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        let guid = extract_guid(&msg);
        assert!(guid.is_none(), "GUID should be None when option is missing");
    }

    #[test]
    fn test_extract_guid_wrong_type_byte() {
        use dhcproto::v4::UnknownOption;

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        // Create Option 97 with wrong type byte (1 instead of 0)
        let mut option_data = vec![1u8]; // Type byte = 1 (wrong)
        option_data.extend_from_slice(&[0u8; 16]); // 16 bytes of zeros

        msg.opts_mut()
            .insert(DhcpOption::Unknown(UnknownOption::new(
                OptionCode::Unknown(97),
                option_data.clone(),
            )));

        let guid = extract_guid(&msg);
        assert!(
            guid.is_none(),
            "GUID should be None when type byte is not 0"
        );
    }

    #[test]
    fn test_extract_guid_wrong_length() {
        use dhcproto::v4::UnknownOption;

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        // Create Option 97 with wrong length (too short)
        let option_data = vec![0u8; 10]; // Only 10 bytes total

        msg.opts_mut()
            .insert(DhcpOption::Unknown(UnknownOption::new(
                OptionCode::Unknown(97),
                option_data.clone(),
            )));

        let guid = extract_guid(&msg);
        assert!(guid.is_none(), "GUID should be None when length is wrong");
    }

    #[test]
    fn test_request_context_includes_guid() {
        use dhcproto::v4::UnknownOption;

        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        msg.opts_mut()
            .insert(DhcpOption::MessageType(MessageType::Discover));

        // Add GUID option
        let mut option_data = vec![0u8]; // Type byte = 0
        option_data.extend_from_slice(&[
            0x00, 0x84, 0x0e, 0x55, 0x9b, 0xe2, 0xd4, 0x41, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]);
        msg.opts_mut()
            .insert(DhcpOption::Unknown(UnknownOption::new(
                OptionCode::Unknown(97),
                option_data.clone(),
            )));

        let ctx = RequestContext::from_message(&msg);
        assert!(ctx.guid.is_some(), "RequestContext should include GUID");
    }

    #[test]
    fn test_request_context_without_guid() {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        msg.opts_mut()
            .insert(DhcpOption::MessageType(MessageType::Discover));

        let ctx = RequestContext::from_message(&msg);
        assert!(
            ctx.guid.is_none(),
            "RequestContext should have None GUID when option is missing"
        );
    }

    #[test]
    fn test_extract_server_identifier_present() {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        let expected_server_id: Ipv4Addr = "10.0.0.1".parse().unwrap();
        msg.opts_mut()
            .insert(DhcpOption::ServerIdentifier(expected_server_id));

        let server_id = extract_server_identifier(&msg);
        assert_eq!(
            server_id,
            Some(expected_server_id),
            "Server Identifier should be extracted when present"
        );
    }

    #[test]
    fn test_extract_server_identifier_missing() {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootRequest);
        msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        msg.opts_mut()
            .insert(DhcpOption::MessageType(MessageType::Request));

        let server_id = extract_server_identifier(&msg);
        assert_eq!(
            server_id, None,
            "Server Identifier should be None when option is missing"
        );
    }

    #[test]
    fn test_extract_server_identifier_from_various_ips() {
        let test_cases = vec![
            "192.168.1.1",
            "10.0.0.254",
            "172.16.0.1",
            "127.0.0.1",
            "255.255.255.255",
        ];

        for ip_str in test_cases {
            let mut msg = Message::default();
            msg.set_opcode(Opcode::BootRequest);
            msg.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

            let expected_ip: Ipv4Addr = ip_str.parse().unwrap();
            msg.opts_mut()
                .insert(DhcpOption::ServerIdentifier(expected_ip));

            let server_id = extract_server_identifier(&msg);
            assert_eq!(
                server_id,
                Some(expected_ip),
                "Server Identifier should correctly extract IP {}",
                ip_str
            );
        }
    }
}
