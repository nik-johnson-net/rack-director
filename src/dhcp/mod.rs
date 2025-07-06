mod packet;
mod server;
mod pool;
mod option82;

#[cfg(test)]
mod tests;

pub use server::DhcpServer;
pub use packet::{DhcpPacket, DhcpMessageType, DhcpOption};
pub use pool::IpPool;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    pub fn new(bytes: [u8; 6]) -> Self {
        MacAddress(bytes)
    }
    
    pub fn from_string(s: &str) -> std::result::Result<Self, String> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return Err("Invalid MAC address format".to_string());
        }
        
        let mut bytes = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            bytes[i] = u8::from_str_radix(part, 16)
                .map_err(|_| "Invalid hex digit in MAC address".to_string())?;
        }
        
        Ok(MacAddress(bytes))
    }
    
    pub fn to_string(&self) -> String {
        format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5])
    }
    
    pub fn bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub id: Option<i32>,
    pub device_id: i32,
    pub mac_address: MacAddress,
    pub ipv4_address: Option<Ipv4Addr>,
    pub ipv6_address: Option<Ipv6Addr>,
    pub is_bmc: bool,
    pub rack_identifier: Option<String>,
    pub rack_port: Option<String>,
    pub subnet_id: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Subnet {
    pub id: Option<i32>,
    pub name: String,
    pub network_ipv4: Option<ipnet::Ipv4Net>,
    pub network_ipv6: Option<ipnet::Ipv6Net>,
    pub gateway_ipv4: Option<Ipv4Addr>,
    pub gateway_ipv6: Option<Ipv6Addr>,
    pub dns_servers: Vec<IpAddr>,
    pub lease_time: u32,
}

#[derive(Debug, Clone)]
pub struct DhcpLease {
    pub id: Option<i32>,
    pub interface_id: i32,
    pub subnet_id: i32,
    pub ip_address: IpAddr,
    pub lease_start: chrono::DateTime<chrono::Utc>,
    pub lease_end: chrono::DateTime<chrono::Utc>,
    pub is_active: bool,
}

pub type Result<T> = std::result::Result<T, DhcpError>;

#[derive(Debug)]
pub enum DhcpError {
    ParseError(String),
    DatabaseError(String),
    NetworkError(String),
    ConfigError(String),
}

impl std::fmt::Display for DhcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DhcpError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            DhcpError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            DhcpError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            DhcpError::ConfigError(msg) => write!(f, "Config error: {}", msg),
        }
    }
}

impl std::error::Error for DhcpError {}

impl From<String> for DhcpError {
    fn from(err: String) -> Self {
        DhcpError::ParseError(err)
    }
}

impl From<&str> for DhcpError {
    fn from(err: &str) -> Self {
        DhcpError::ParseError(err.to_string())
    }
}