use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use tokio::net::UdpSocket;

use crate::tftp::{
    packet::{Error, Packet},
    state::{ControlFlow, State},
};

mod packet;
mod state;

pub trait Handler {
    async fn read(&self, filename: &String) -> Result<Vec<u8>>;
}

pub struct Server<H: Handler> {
    address: String,
    handler: H,
}

impl<H: Handler + 'static> Server<H> {
    pub fn new(handler: H) -> Self {
        Self {
            address: "0.0.0.0:57".to_owned(),
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

struct Router<H: Handler> {
    handler: Arc<H>,
    socket: Arc<UdpSocket>,
    connections: HashMap<SocketAddr, State<H>>,
}

impl<H: Handler + 'static> Router<H> {
    fn new(socket: Arc<UdpSocket>, handler: Arc<H>) -> Self {
        Self {
            handler,
            socket,
            connections: HashMap::new(),
        }
    }

    async fn route(&mut self, addr: SocketAddr, packet: Packet) -> Result<()> {
        // If the transfer is not known, detect if it's a new transfer (Rrq or Wrq). If so, start a new session. Otherwise, return an error.
        if !self.connections.contains_key(&addr) {
            if packet.can_initiate() {
                self.connections
                    .insert(addr, State::new(addr, self.handler.clone()));
            } else {
                // Send unknown transfer back to client.
                let response = Packet::Error {
                    code: Error::UnknownTransferID,
                    message: String::new(),
                };
                self.socket.send_to(&response.to_bytes(), addr).await?;
            }
        }

        // Delegate control to the state object
        let state = self.connections.get_mut(&addr).unwrap();
        let control_flow = state.handle(packet).await;
        drop(state);
        
        // Handle the response from the state object.
        // First determine if the state object is "complete" and can be removed,
        // then send the packet.
        let packet_opt = match control_flow {
            ControlFlow::Continue(packet) => Some(packet),
            ControlFlow::Closed(packet) => {
                self.connections.remove(&addr);
                packet
            },
        };

        if let Some(packet) = packet_opt {
            self.socket.send_to(&packet.to_bytes(), addr).await?;
        }

        Ok(())
    }
}

async fn serve<H: Handler + 'static>(socket: UdpSocket, handler: H) -> Result<()> {
    let arc_socket = Arc::new(socket);
    let arc_handler = Arc::new(handler);
    let mut router = Router::new(arc_socket.clone(), arc_handler);
    let mut buf: [u8; 512] = [0; 512];

    loop {
        let (size, addr) = arc_socket.recv_from(&mut buf).await?;
        let packet = Packet::parse(&buf[0..size])?;
        router.route(addr, packet).await?;
    }
}
