#[cfg(test)]
mod tests {
    use crate::dhcp::{DhcpServer, MacAddress, packet::DhcpPacket, pool::IpPool, Subnet};
    use std::net::Ipv4Addr;
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use std::sync::Arc;

    fn setup_test_db() -> Arc<Mutex<rusqlite::Connection>> {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(&db_path).unwrap();
        Arc::new(Mutex::new(conn))
    }
    
    async fn create_test_subnet(db: &Arc<Mutex<rusqlite::Connection>>) -> i32 {
        let conn = db.lock().await;
        conn.execute(
            "INSERT INTO subnets (name, network_ipv4, gateway_ipv4, dns_servers, lease_time) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["test-subnet", "192.168.1.0/24", "192.168.1.1", "[]", 3600]
        ).unwrap();
        conn.last_insert_rowid() as i32
    }

    #[tokio::test]
    async fn test_dhcp_server_creation() {
        let db = setup_test_db();
        let server_ip = Ipv4Addr::new(192, 168, 1, 1);
        let _server = DhcpServer::new(db, server_ip, None);
        
        // Should not panic - just test creation
        assert!(true);
    }

    #[test]
    fn test_dhcp_packet_parsing() {
        let mut packet = DhcpPacket::new();
        packet.op = 1; // BOOTREQUEST
        packet.xid = 0x12345678;
        packet.chaddr = MacAddress::from_string("00:11:22:33:44:55").unwrap();
        
        let serialized = packet.serialize();
        let parsed = DhcpPacket::parse(&serialized).unwrap();
        
        assert_eq!(parsed.op, 1);
        assert_eq!(parsed.xid, 0x12345678);
        assert_eq!(parsed.chaddr.to_string(), "00:11:22:33:44:55");
    }

    #[test]
    fn test_ip_pool_allocation() {
        let mut pool = IpPool::new();
        
        let subnet = Subnet {
            id: Some(1),
            name: "test".to_string(),
            network_ipv4: Some("192.168.1.0/24".parse().unwrap()),
            network_ipv6: None,
            gateway_ipv4: Some(Ipv4Addr::new(192, 168, 1, 1)),
            gateway_ipv6: None,
            dns_servers: Vec::new(),
            lease_time: 3600,
        };
        
        pool.add_subnet(&subnet).unwrap();
        
        let ip1 = pool.allocate_ipv4(Some(1));
        assert!(ip1.is_some());
        
        let ip2 = pool.allocate_ipv4(Some(1));
        assert!(ip2.is_some());
        assert_ne!(ip1, ip2);
    }

    #[test]
    fn test_mac_address_parsing() {
        let mac = MacAddress::from_string("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(mac.to_string(), "aa:bb:cc:dd:ee:ff");
        
        let invalid_mac = MacAddress::from_string("invalid");
        assert!(invalid_mac.is_err());
    }

    #[test] 
    fn test_option82_parsing() {
        use crate::dhcp::option82::Option82Parser;
        use crate::dhcp::packet::Option82Data;
        
        let option82 = Option82Data {
            circuit_id: Some(b"rack-01:port-24".to_vec()),
            remote_id: Some(b"switch-01".to_vec()),
        };
        
        let result = Option82Parser::parse_rack_info(&option82);
        assert_eq!(result, Some(("rack-01".to_string(), "port-24".to_string())));
    }
}