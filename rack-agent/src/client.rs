use anyhow::Result;
use common::DeviceScan;

pub struct RackDirector {
    // client: reqwest::Client;
}

impl RackDirector {
    pub fn new(_url: &str) -> RackDirector {
        RackDirector {}
    }

    pub async fn PostDeviceScan(_payload: DeviceScan) -> Result<()> {
        unimplemented!()
    }
}
