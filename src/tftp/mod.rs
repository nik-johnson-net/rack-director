use std::sync::Arc;

use anyhow::Result;
use tokio::net::UdpSocket;

use crate::tftp::{connection::Connection, packet::Packet};

mod connection;
mod packet;
mod state;
pub use state::Handler;
pub use state::Reader;

pub struct Server<H: Handler> {
    address: String,
    handler: H,
}

impl<H: Handler + Send + Sync + 'static> Server<H> {
    pub fn new(handler: H) -> Self {
        Self {
            address: "0.0.0.0:69".to_owned(),
            handler,
        }
    }

    #[allow(unused)]
    pub fn address(&mut self, addr: String) -> &mut Self {
        self.address = addr;
        self
    }

    pub async fn serve(self) -> Result<()> {
        let socket = tokio::net::UdpSocket::bind(self.address).await?;
        serve(socket, self.handler).await?;
        Ok(())
    }
}

async fn serve<H: Handler + Send + Sync + 'static>(socket: UdpSocket, handler: H) -> Result<()> {
    let arc_socket = Arc::new(socket);
    let arc_handler = Arc::new(handler);
    let mut buf: [u8; 512] = [0; 512];

    loop {
        let (size, addr) = arc_socket.recv_from(&mut buf).await?;
        let packet = Packet::parse(&buf[0..size])?;
        tokio::spawn(Connection::accept(arc_handler.clone(), addr, packet));
    }
}
