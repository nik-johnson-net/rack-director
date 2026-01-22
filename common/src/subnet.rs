use std::{error::Error, fmt::Display, net::Ipv4Addr, str::FromStr};

#[derive(Debug)]
pub enum Ipv4SubnetError {
    InvalidCidr,
    InvalidPrefix,
}

impl Error for Ipv4SubnetError {}

impl Display for Ipv4SubnetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ipv4SubnetError::InvalidCidr => {
                f.write_str("Invalid CIDR format, must be of the form '192.168.0.1/24'")
            }
            Ipv4SubnetError::InvalidPrefix => {
                f.write_str("Prefix must be equal to or shorter than 32")
            }
        }
    }
}

pub struct Ipv4Subnet {
    pub addr: Ipv4Addr,
    subnet: u8,
}

impl Ipv4Subnet {
    pub fn new(addr: Ipv4Addr, subnet: u8) -> Self {
        Self { addr, subnet }
    }

    pub fn netmask(&self) -> Ipv4Addr {
        let init: u32 = 0xFF_FF_FF_FF;
        let mask = init.unbounded_shl((32 - self.subnet).into());
        Ipv4Addr::from_bits(mask)
    }

    pub fn subnet(&self) -> u8 {
        self.subnet
    }

    pub fn set_subnet(&mut self, subnet: u8) {
        self.subnet = subnet;
    }

    pub fn network(&self) -> Ipv4Addr {
        self.addr & self.netmask()
    }

    pub fn ip_in_range(&self, ip: Ipv4Addr) -> bool {
        let netmask_bits = self.netmask().to_bits();
        (ip.to_bits() & netmask_bits) == self.network().to_bits()
    }
}

impl FromStr for Ipv4Subnet {
    type Err = Ipv4SubnetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let addr: Ipv4Addr;
        let subnet: u8;

        if let Some((l, r)) = s.split_once('/') {
            match l.parse::<Ipv4Addr>() {
                Ok(parsed_addr) => addr = parsed_addr,
                Err(_) => {
                    return Err(Ipv4SubnetError::InvalidCidr);
                }
            };

            match r.parse::<u8>() {
                Ok(parsed_subnet) => subnet = parsed_subnet,
                Err(_) => {
                    return Err(Ipv4SubnetError::InvalidCidr);
                }
            };

            if subnet > 32 {
                return Err(Ipv4SubnetError::InvalidPrefix);
            }

            Ok(Self { addr, subnet })
        } else {
            Err(Ipv4SubnetError::InvalidCidr)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use crate::Ipv4Subnet;

    #[test]
    fn ipv4subnet_netmask() {
        let mut subnet = Ipv4Subnet::new(Ipv4Addr::new(127, 0, 0, 1), 24);
        assert_eq!(subnet.netmask(), Ipv4Addr::new(255, 255, 255, 0));

        subnet.subnet = 31;
        assert_eq!(subnet.netmask(), Ipv4Addr::new(255, 255, 255, 254));

        subnet.subnet = 0;
        assert_eq!(subnet.netmask(), Ipv4Addr::new(0, 0, 0, 0));
    }

    #[test]
    fn ipv4subnet_network() {
        let subnet = Ipv4Subnet::new(Ipv4Addr::new(192, 168, 0, 15), 24);
        assert_eq!(subnet.network(), Ipv4Addr::new(192, 168, 0, 0));
    }

    #[test]
    fn ip_in_range() {
        let subnet = Ipv4Subnet::new(Ipv4Addr::new(192, 168, 0, 15), 24);
        assert!(subnet.ip_in_range(Ipv4Addr::new(192, 168, 0, 100)));
        assert!(subnet.ip_in_range(Ipv4Addr::new(192, 168, 0, 0)));
        assert!(subnet.ip_in_range(Ipv4Addr::new(192, 168, 0, 255)));
        assert!(!subnet.ip_in_range(Ipv4Addr::new(192, 168, 1, 100)));
        assert!(!subnet.ip_in_range(Ipv4Addr::new(192, 167, 0, 255)));
    }
}
