use std::{error::Error, fmt::Display, net::Ipv4Addr, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceScan {
    pub uuid: String,
    pub attributes: Vec<DeviceAttribute>,
}

#[derive(Debug)]
pub struct Ipv4SubnetError {
    msg: String,
}

impl Ipv4SubnetError {
    fn new(msg: String) -> Self {
        Self { msg }
    }
}

impl Error for Ipv4SubnetError {}

impl Display for Ipv4SubnetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
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
                    return Err(Ipv4SubnetError::new(
                        "ipv4 subnet must be of the form 0.0.0.0/0".to_string(),
                    ));
                }
            };

            match r.parse::<u8>() {
                Ok(parsed_subnet) => subnet = parsed_subnet,
                Err(_) => {
                    return Err(Ipv4SubnetError::new(
                        "ipv4 subnet must be of the form 0.0.0.0/0".to_string(),
                    ));
                }
            };

            if subnet > 32 {
                return Err(Ipv4SubnetError::new(
                    "subnet must be between 0 and 32".to_string(),
                ));
            }

            Ok(Self { addr, subnet })
        } else {
            Err(Ipv4SubnetError::new(
                "ipv4 subnet must be of the form 0.0.0.0/0".to_string(),
            ))
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
}
