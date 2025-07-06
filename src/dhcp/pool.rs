use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::collections::HashSet;
use ipnet::{Ipv4Net, Ipv6Net};
use rand::{Rng, random};
use crate::dhcp::{Result, Subnet};

pub struct IpPool {
    ipv4_pools: Vec<Ipv4Pool>,
    ipv6_pools: Vec<Ipv6Pool>,
}

impl IpPool {
    pub fn new() -> Self {
        IpPool {
            ipv4_pools: Vec::new(),
            ipv6_pools: Vec::new(),
        }
    }
    
    pub fn add_subnet(&mut self, subnet: &Subnet) -> Result<()> {
        if let Some(ipv4_net) = &subnet.network_ipv4 {
            self.ipv4_pools.push(Ipv4Pool::new(*ipv4_net, subnet.id.unwrap_or(0)));
        }
        
        if let Some(ipv6_net) = &subnet.network_ipv6 {
            self.ipv6_pools.push(Ipv6Pool::new(*ipv6_net, subnet.id.unwrap_or(0)));
        }
        
        Ok(())
    }
    
    pub fn allocate_ipv4(&mut self, subnet_id: Option<i32>) -> Option<Ipv4Addr> {
        if let Some(id) = subnet_id {
            // Try to allocate from specific subnet
            if let Some(pool) = self.ipv4_pools.iter_mut().find(|p| p.subnet_id == id) {
                return pool.allocate();
            }
        } else {
            // Try to allocate from any available pool
            for pool in &mut self.ipv4_pools {
                if let Some(ip) = pool.allocate() {
                    return Some(ip);
                }
            }
        }
        None
    }
    
    pub fn allocate_ipv6(&mut self, subnet_id: Option<i32>) -> Option<Ipv6Addr> {
        if let Some(id) = subnet_id {
            // Try to allocate from specific subnet
            if let Some(pool) = self.ipv6_pools.iter_mut().find(|p| p.subnet_id == id) {
                return pool.allocate();
            }
        } else {
            // Try to allocate from any available pool
            for pool in &mut self.ipv6_pools {
                if let Some(ip) = pool.allocate() {
                    return Some(ip);
                }
            }
        }
        None
    }
    
    pub fn release_ip(&mut self, ip: IpAddr) {
        match ip {
            IpAddr::V4(ipv4) => {
                for pool in &mut self.ipv4_pools {
                    pool.release(ipv4);
                }
            },
            IpAddr::V6(ipv6) => {
                for pool in &mut self.ipv6_pools {
                    pool.release(ipv6);
                }
            },
        }
    }
    
    pub fn mark_used(&mut self, ip: IpAddr) {
        match ip {
            IpAddr::V4(ipv4) => {
                for pool in &mut self.ipv4_pools {
                    if pool.network.contains(&ipv4) {
                        pool.mark_used(ipv4);
                        break;
                    }
                }
            },
            IpAddr::V6(ipv6) => {
                for pool in &mut self.ipv6_pools {
                    if pool.network.contains(&ipv6) {
                        pool.mark_used(ipv6);
                        break;
                    }
                }
            },
        }
    }
    
    pub fn is_available(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                for pool in &self.ipv4_pools {
                    if pool.network.contains(&ipv4) {
                        return !pool.allocated.contains(&ipv4);
                    }
                }
                false
            },
            IpAddr::V6(ipv6) => {
                for pool in &self.ipv6_pools {
                    if pool.network.contains(&ipv6) {
                        return !pool.allocated.contains(&ipv6);
                    }
                }
                false
            },
        }
    }
}

struct Ipv4Pool {
    network: Ipv4Net,
    subnet_id: i32,
    allocated: HashSet<Ipv4Addr>,
}

impl Ipv4Pool {
    fn new(network: Ipv4Net, subnet_id: i32) -> Self {
        Ipv4Pool {
            network,
            subnet_id,
            allocated: HashSet::new(),
        }
    }
    
    fn allocate(&mut self) -> Option<Ipv4Addr> {
        let network_addr = self.network.network();
        let broadcast_addr = self.network.broadcast();
        
        // Skip network and broadcast addresses
        let start_ip = u32::from(network_addr) + 1;
        let end_ip = u32::from(broadcast_addr) - 1;
        
        if start_ip >= end_ip {
            return None;
        }
        
        // Try random allocation first (more efficient for large subnets)
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let ip_u32 = rng.gen_range(start_ip..=end_ip);
            let ip = Ipv4Addr::from(ip_u32);
            
            if !self.allocated.contains(&ip) {
                self.allocated.insert(ip);
                return Some(ip);
            }
        }
        
        // Fall back to sequential allocation
        for ip_u32 in start_ip..=end_ip {
            let ip = Ipv4Addr::from(ip_u32);
            if !self.allocated.contains(&ip) {
                self.allocated.insert(ip);
                return Some(ip);
            }
        }
        
        None
    }
    
    fn release(&mut self, ip: Ipv4Addr) {
        self.allocated.remove(&ip);
    }
    
    fn mark_used(&mut self, ip: Ipv4Addr) {
        self.allocated.insert(ip);
    }
}

struct Ipv6Pool {
    network: Ipv6Net,
    subnet_id: i32,
    allocated: HashSet<Ipv6Addr>,
}

impl Ipv6Pool {
    fn new(network: Ipv6Net, subnet_id: i32) -> Self {
        Ipv6Pool {
            network,
            subnet_id,
            allocated: HashSet::new(),
        }
    }
    
    fn allocate(&mut self) -> Option<Ipv6Addr> {
        // For IPv6, we'll use a simpler approach due to the large address space
        // Generate random addresses in the subnet
        let network_addr = self.network.network();
        let prefix_len = self.network.prefix_len();
        
        if prefix_len >= 128 {
            return None;
        }
        
        let network_bytes = network_addr.octets();
        let mut rng = rand::thread_rng();
        
        // Try to generate a random address in the subnet
        for _ in 0..1000 {
            let mut addr_bytes = network_bytes;
            
            // Randomize the host part
            let host_bits = 128 - prefix_len;
            let host_bytes = (host_bits + 7) / 8;
            
            for i in 0..host_bytes {
                let byte_idx = 16 - host_bytes as usize + i as usize;
                if byte_idx < 16 {
                    addr_bytes[byte_idx] = random::<u8>();
                }
            }
            
            // Clear network bits to ensure we're in the correct subnet
            let network_bytes_to_clear = prefix_len / 8;
            for i in 0..network_bytes_to_clear {
                addr_bytes[i as usize] = network_bytes[i as usize];
            }
            
            // Handle partial byte
            if prefix_len % 8 != 0 {
                let byte_idx = (prefix_len / 8) as usize;
                if byte_idx < 16 {
                    let mask = 0xFF << (8 - (prefix_len % 8));
                    addr_bytes[byte_idx] = (addr_bytes[byte_idx] & !mask) | (network_bytes[byte_idx] & mask);
                }
            }
            
            let ip = Ipv6Addr::from(addr_bytes);
            
            if self.network.contains(&ip) && !self.allocated.contains(&ip) {
                self.allocated.insert(ip);
                return Some(ip);
            }
        }
        
        None
    }
    
    fn release(&mut self, ip: Ipv6Addr) {
        self.allocated.remove(&ip);
    }
    
    fn mark_used(&mut self, ip: Ipv6Addr) {
        self.allocated.insert(ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ipv4_pool_allocation() {
        let network = "192.168.1.0/24".parse::<Ipv4Net>().unwrap();
        let mut pool = Ipv4Pool::new(network, 1);
        
        let ip1 = pool.allocate();
        assert!(ip1.is_some());
        
        let ip2 = pool.allocate();
        assert!(ip2.is_some());
        assert_ne!(ip1, ip2);
        
        // Release and reallocate
        pool.release(ip1.unwrap());
        let ip3 = pool.allocate();
        assert!(ip3.is_some());
    }
    
    #[test]
    fn test_ipv6_pool_allocation() {
        let network = "2001:db8::/64".parse::<Ipv6Net>().unwrap();
        let mut pool = Ipv6Pool::new(network, 1);
        
        let ip1 = pool.allocate();
        assert!(ip1.is_some());
        
        let ip2 = pool.allocate();
        assert!(ip2.is_some());
        assert_ne!(ip1, ip2);
        
        // Check that allocated IPs are in the correct subnet
        assert!(network.contains(&ip1.unwrap()));
        assert!(network.contains(&ip2.unwrap()));
    }
    
    #[test]
    fn test_ip_pool_management() {
        let mut pool = IpPool::new();
        
        let subnet = Subnet {
            id: Some(1),
            name: "test".to_string(),
            network_ipv4: Some("192.168.1.0/24".parse().unwrap()),
            network_ipv6: Some("2001:db8::/64".parse().unwrap()),
            gateway_ipv4: None,
            gateway_ipv6: None,
            dns_servers: Vec::new(),
            lease_time: 3600,
        };
        
        pool.add_subnet(&subnet).unwrap();
        
        let ipv4 = pool.allocate_ipv4(Some(1));
        assert!(ipv4.is_some());
        
        let ipv6 = pool.allocate_ipv6(Some(1));
        assert!(ipv6.is_some());
        
        // Test release
        pool.release_ip(IpAddr::V4(ipv4.unwrap()));
        assert!(pool.is_available(IpAddr::V4(ipv4.unwrap())));
    }
}