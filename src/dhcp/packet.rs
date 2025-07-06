use std::net::{Ipv4Addr, Ipv6Addr};
use std::collections::HashMap;
use crate::dhcp::MacAddress;

#[derive(Debug, Clone, PartialEq)]
pub enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
}

impl TryFrom<u8> for DhcpMessageType {
    type Error = String;
    
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DhcpMessageType::Discover),
            2 => Ok(DhcpMessageType::Offer),
            3 => Ok(DhcpMessageType::Request),
            4 => Ok(DhcpMessageType::Decline),
            5 => Ok(DhcpMessageType::Ack),
            6 => Ok(DhcpMessageType::Nak),
            7 => Ok(DhcpMessageType::Release),
            8 => Ok(DhcpMessageType::Inform),
            _ => Err(format!("Unknown DHCP message type: {}", value)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DhcpOption {
    SubnetMask(Ipv4Addr),
    Router(Vec<Ipv4Addr>),
    DnsServers(Vec<Ipv4Addr>),
    DomainName(String),
    LeaseTime(u32),
    MessageType(DhcpMessageType),
    ServerIdentifier(Ipv4Addr),
    RequestedIpAddress(Ipv4Addr),
    ClientIdentifier(Vec<u8>),
    Option82(Option82Data),
    Other(u8, Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Option82Data {
    pub circuit_id: Option<Vec<u8>>,
    pub remote_id: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct DhcpPacket {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub hops: u8,
    pub xid: u32,
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: Ipv4Addr,
    pub yiaddr: Ipv4Addr,
    pub siaddr: Ipv4Addr,
    pub giaddr: Ipv4Addr,
    pub chaddr: MacAddress,
    pub sname: [u8; 64],
    pub file: [u8; 128],
    pub options: HashMap<u8, DhcpOption>,
}

impl DhcpPacket {
    pub fn new() -> Self {
        DhcpPacket {
            op: 0,
            htype: 1, // Ethernet
            hlen: 6,  // MAC address length
            hops: 0,
            xid: 0,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::new(0, 0, 0, 0),
            yiaddr: Ipv4Addr::new(0, 0, 0, 0),
            siaddr: Ipv4Addr::new(0, 0, 0, 0),
            giaddr: Ipv4Addr::new(0, 0, 0, 0),
            chaddr: MacAddress::new([0; 6]),
            sname: [0; 64],
            file: [0; 128],
            options: HashMap::new(),
        }
    }
    
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 236 {
            return Err("DHCP packet too short".to_string());
        }
        
        let mut packet = DhcpPacket::new();
        
        packet.op = data[0];
        packet.htype = data[1];
        packet.hlen = data[2];
        packet.hops = data[3];
        packet.xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        packet.secs = u16::from_be_bytes([data[8], data[9]]);
        packet.flags = u16::from_be_bytes([data[10], data[11]]);
        
        packet.ciaddr = Ipv4Addr::from([data[12], data[13], data[14], data[15]]);
        packet.yiaddr = Ipv4Addr::from([data[16], data[17], data[18], data[19]]);
        packet.siaddr = Ipv4Addr::from([data[20], data[21], data[22], data[23]]);
        packet.giaddr = Ipv4Addr::from([data[24], data[25], data[26], data[27]]);
        
        let mut chaddr = [0u8; 6];
        chaddr.copy_from_slice(&data[28..34]);
        packet.chaddr = MacAddress::new(chaddr);
        
        packet.sname.copy_from_slice(&data[44..108]);
        packet.file.copy_from_slice(&data[108..236]);
        
        // Parse options
        if data.len() > 236 && data[236..240] == [0x63, 0x82, 0x53, 0x63] {
            packet.options = Self::parse_options(&data[240..])?;
        }
        
        Ok(packet)
    }
    
    fn parse_options(data: &[u8]) -> Result<HashMap<u8, DhcpOption>, String> {
        let mut options = HashMap::new();
        let mut i = 0;
        
        while i < data.len() {
            if data[i] == 255 { // End option
                break;
            }
            
            if data[i] == 0 { // Pad option
                i += 1;
                continue;
            }
            
            if i + 1 >= data.len() {
                return Err("Invalid option format".to_string());
            }
            
            let option_code = data[i];
            let option_len = data[i + 1] as usize;
            
            if i + 2 + option_len > data.len() {
                return Err("Option length exceeds packet size".to_string());
            }
            
            let option_data = &data[i + 2..i + 2 + option_len];
            
            let option = match option_code {
                1 => {
                    if option_len == 4 {
                        DhcpOption::SubnetMask(Ipv4Addr::from([
                            option_data[0], option_data[1], option_data[2], option_data[3]
                        ]))
                    } else {
                        DhcpOption::Other(option_code, option_data.to_vec())
                    }
                },
                3 => {
                    let mut routers = Vec::new();
                    for chunk in option_data.chunks(4) {
                        if chunk.len() == 4 {
                            routers.push(Ipv4Addr::from([chunk[0], chunk[1], chunk[2], chunk[3]]));
                        }
                    }
                    DhcpOption::Router(routers)
                },
                6 => {
                    let mut dns_servers = Vec::new();
                    for chunk in option_data.chunks(4) {
                        if chunk.len() == 4 {
                            dns_servers.push(Ipv4Addr::from([chunk[0], chunk[1], chunk[2], chunk[3]]));
                        }
                    }
                    DhcpOption::DnsServers(dns_servers)
                },
                15 => DhcpOption::DomainName(String::from_utf8_lossy(option_data).to_string()),
                51 => {
                    if option_len == 4 {
                        DhcpOption::LeaseTime(u32::from_be_bytes([
                            option_data[0], option_data[1], option_data[2], option_data[3]
                        ]))
                    } else {
                        DhcpOption::Other(option_code, option_data.to_vec())
                    }
                },
                53 => {
                    if option_len == 1 {
                        match DhcpMessageType::try_from(option_data[0]) {
                            Ok(msg_type) => DhcpOption::MessageType(msg_type),
                            Err(_) => DhcpOption::Other(option_code, option_data.to_vec()),
                        }
                    } else {
                        DhcpOption::Other(option_code, option_data.to_vec())
                    }
                },
                54 => {
                    if option_len == 4 {
                        DhcpOption::ServerIdentifier(Ipv4Addr::from([
                            option_data[0], option_data[1], option_data[2], option_data[3]
                        ]))
                    } else {
                        DhcpOption::Other(option_code, option_data.to_vec())
                    }
                },
                50 => {
                    if option_len == 4 {
                        DhcpOption::RequestedIpAddress(Ipv4Addr::from([
                            option_data[0], option_data[1], option_data[2], option_data[3]
                        ]))
                    } else {
                        DhcpOption::Other(option_code, option_data.to_vec())
                    }
                },
                61 => DhcpOption::ClientIdentifier(option_data.to_vec()),
                82 => DhcpOption::Option82(Self::parse_option82(option_data)?),
                _ => DhcpOption::Other(option_code, option_data.to_vec()),
            };
            
            options.insert(option_code, option);
            i += 2 + option_len;
        }
        
        Ok(options)
    }
    
    fn parse_option82(data: &[u8]) -> Result<Option82Data, String> {
        let mut option82 = Option82Data {
            circuit_id: None,
            remote_id: None,
        };
        
        let mut i = 0;
        while i < data.len() {
            if i + 1 >= data.len() {
                break;
            }
            
            let subcode = data[i];
            let sublen = data[i + 1] as usize;
            
            if i + 2 + sublen > data.len() {
                break;
            }
            
            let subdata = &data[i + 2..i + 2 + sublen];
            
            match subcode {
                1 => option82.circuit_id = Some(subdata.to_vec()),
                2 => option82.remote_id = Some(subdata.to_vec()),
                _ => {}, // Ignore unknown subcodes
            }
            
            i += 2 + sublen;
        }
        
        Ok(option82)
    }
    
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(576);
        
        data.push(self.op);
        data.push(self.htype);
        data.push(self.hlen);
        data.push(self.hops);
        data.extend_from_slice(&self.xid.to_be_bytes());
        data.extend_from_slice(&self.secs.to_be_bytes());
        data.extend_from_slice(&self.flags.to_be_bytes());
        
        data.extend_from_slice(&self.ciaddr.octets());
        data.extend_from_slice(&self.yiaddr.octets());
        data.extend_from_slice(&self.siaddr.octets());
        data.extend_from_slice(&self.giaddr.octets());
        
        data.extend_from_slice(self.chaddr.bytes());
        data.extend_from_slice(&[0; 10]); // Padding for chaddr
        
        data.extend_from_slice(&self.sname);
        data.extend_from_slice(&self.file);
        
        // DHCP magic cookie
        data.extend_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        
        // Serialize options
        for (code, option) in &self.options {
            self.serialize_option(&mut data, *code, option);
        }
        
        // End option
        data.push(255);
        
        data
    }
    
    fn serialize_option(&self, data: &mut Vec<u8>, code: u8, option: &DhcpOption) {
        data.push(code);
        
        match option {
            DhcpOption::SubnetMask(addr) => {
                data.push(4);
                data.extend_from_slice(&addr.octets());
            },
            DhcpOption::Router(routers) => {
                data.push((routers.len() * 4) as u8);
                for router in routers {
                    data.extend_from_slice(&router.octets());
                }
            },
            DhcpOption::DnsServers(servers) => {
                data.push((servers.len() * 4) as u8);
                for server in servers {
                    data.extend_from_slice(&server.octets());
                }
            },
            DhcpOption::DomainName(name) => {
                data.push(name.len() as u8);
                data.extend_from_slice(name.as_bytes());
            },
            DhcpOption::LeaseTime(time) => {
                data.push(4);
                data.extend_from_slice(&time.to_be_bytes());
            },
            DhcpOption::MessageType(msg_type) => {
                data.push(1);
                data.push(msg_type.clone() as u8);
            },
            DhcpOption::ServerIdentifier(addr) => {
                data.push(4);
                data.extend_from_slice(&addr.octets());
            },
            DhcpOption::RequestedIpAddress(addr) => {
                data.push(4);
                data.extend_from_slice(&addr.octets());
            },
            DhcpOption::ClientIdentifier(id) => {
                data.push(id.len() as u8);
                data.extend_from_slice(id);
            },
            DhcpOption::Option82(opt82) => {
                let mut opt82_data = Vec::new();
                if let Some(circuit_id) = &opt82.circuit_id {
                    opt82_data.push(1);
                    opt82_data.push(circuit_id.len() as u8);
                    opt82_data.extend_from_slice(circuit_id);
                }
                if let Some(remote_id) = &opt82.remote_id {
                    opt82_data.push(2);
                    opt82_data.push(remote_id.len() as u8);
                    opt82_data.extend_from_slice(remote_id);
                }
                data.push(opt82_data.len() as u8);
                data.extend_from_slice(&opt82_data);
            },
            DhcpOption::Other(_, bytes) => {
                data.push(bytes.len() as u8);
                data.extend_from_slice(bytes);
            },
        }
    }
    
    pub fn get_message_type(&self) -> Option<DhcpMessageType> {
        if let Some(DhcpOption::MessageType(msg_type)) = self.options.get(&53) {
            Some(msg_type.clone())
        } else {
            None
        }
    }
    
    pub fn set_message_type(&mut self, msg_type: DhcpMessageType) {
        self.options.insert(53, DhcpOption::MessageType(msg_type));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mac_address_from_string() {
        let mac = MacAddress::from_string("00:11:22:33:44:55").unwrap();
        assert_eq!(mac.bytes(), &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(mac.to_string(), "00:11:22:33:44:55");
    }
    
    #[test]
    fn test_packet_creation() {
        let mut packet = DhcpPacket::new();
        packet.set_message_type(DhcpMessageType::Discover);
        
        assert_eq!(packet.get_message_type(), Some(DhcpMessageType::Discover));
    }
    
    #[test]
    fn test_packet_serialization() {
        let mut packet = DhcpPacket::new();
        packet.op = 1;
        packet.xid = 0x12345678;
        packet.set_message_type(DhcpMessageType::Discover);
        
        let serialized = packet.serialize();
        assert!(serialized.len() > 240);
        
        let parsed = DhcpPacket::parse(&serialized).unwrap();
        assert_eq!(parsed.op, 1);
        assert_eq!(parsed.xid, 0x12345678);
        assert_eq!(parsed.get_message_type(), Some(DhcpMessageType::Discover));
    }
}