use crate::tftp::Handler;
use crate::tftp::Reader;

pub struct DirectorTftpHandler {
    // Fields for the DirectorTftpHandler
}

impl Handler for DirectorTftpHandler {
    type Reader = DirectorTftpReader;

    async fn create_reader(&self, filename: &str) -> anyhow::Result<Self::Reader> {
        todo!()
    }
}

pub struct DirectorTftpReader {}

impl Reader for DirectorTftpReader {
    async fn read(&mut self) -> anyhow::Result<Vec<u8>> {
        todo!()
    }
}
