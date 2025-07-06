use crate::dhcp::packet::Option82Data;

pub struct Option82Parser;

impl Option82Parser {
    pub fn parse_rack_info(option82: &Option82Data) -> Option<(String, String)> {
        let circuit_id = option82.circuit_id.as_ref()?;
        let remote_id = option82.remote_id.as_ref()?;
        
        // Parse circuit ID to extract rack identifier and port
        // Format: rack_id:port_id
        let circuit_str = String::from_utf8_lossy(circuit_id);
        let parts: Vec<&str> = circuit_str.split(':').collect();
        
        if parts.len() >= 2 {
            let rack_id = parts[0].to_string();
            let port_id = parts[1].to_string();
            return Some((rack_id, port_id));
        }
        
        // Alternative parsing: try to extract from remote ID
        let remote_str = String::from_utf8_lossy(remote_id);
        if remote_str.contains(':') {
            let parts: Vec<&str> = remote_str.split(':').collect();
            if parts.len() >= 2 {
                return Some((parts[0].to_string(), parts[1].to_string()));
            }
        }
        
        None
    }
    
    pub fn extract_switch_info(option82: &Option82Data) -> Option<SwitchInfo> {
        let circuit_id = option82.circuit_id.as_ref()?;
        
        // Try to parse various formats commonly used by switches
        let circuit_str = String::from_utf8_lossy(circuit_id);
        
        // Format 1: switch_hostname:port_number
        if let Some((switch, port)) = Self::parse_hostname_port(&circuit_str) {
            return Some(SwitchInfo {
                switch_identifier: switch,
                port_identifier: port,
                vlan_id: None,
            });
        }
        
        // Format 2: vlan_id:port_number
        if let Some((vlan, port)) = Self::parse_vlan_port(&circuit_str) {
            return Some(SwitchInfo {
                switch_identifier: String::new(),
                port_identifier: port,
                vlan_id: Some(vlan),
            });
        }
        
        None
    }
    
    fn parse_hostname_port(circuit_str: &str) -> Option<(String, String)> {
        let parts: Vec<&str> = circuit_str.split(':').collect();
        if parts.len() == 2 {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }
    
    fn parse_vlan_port(circuit_str: &str) -> Option<(u16, String)> {
        let parts: Vec<&str> = circuit_str.split(':').collect();
        if parts.len() == 2 {
            if let Ok(vlan) = parts[0].parse::<u16>() {
                return Some((vlan, parts[1].to_string()));
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct SwitchInfo {
    pub switch_identifier: String,
    pub port_identifier: String,
    pub vlan_id: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_rack_info() {
        let option82 = Option82Data {
            circuit_id: Some(b"rack-01:port-24".to_vec()),
            remote_id: Some(b"switch-01".to_vec()),
        };
        
        let result = Option82Parser::parse_rack_info(&option82);
        assert_eq!(result, Some(("rack-01".to_string(), "port-24".to_string())));
    }
    
    #[test]
    fn test_extract_switch_info() {
        let option82 = Option82Data {
            circuit_id: Some(b"switch-01:ge-0/0/24".to_vec()),
            remote_id: Some(b"remote-switch".to_vec()),
        };
        
        let result = Option82Parser::extract_switch_info(&option82);
        assert!(result.is_some());
        
        let switch_info = result.unwrap();
        assert_eq!(switch_info.switch_identifier, "switch-01");
        assert_eq!(switch_info.port_identifier, "ge-0/0/24");
    }
}