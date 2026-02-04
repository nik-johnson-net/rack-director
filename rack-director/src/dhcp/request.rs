use dhcproto::v4::{Architecture, DhcpOption, Message, MessageType, OptionCode};
use std::net::Ipv4Addr;

use super::store::format_mac;

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
        }
    }
}
