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
