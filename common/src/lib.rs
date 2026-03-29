pub mod cnc;
pub mod device_attributes;
pub mod disk_layout;
pub mod firmware_mode;
pub mod poll_action;
mod subnet;

pub use firmware_mode::FirmwareMode;
pub use subnet::Ipv4Subnet;
pub use subnet::Ipv4SubnetError;

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
