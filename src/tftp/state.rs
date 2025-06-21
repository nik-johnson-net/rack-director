use std::{net::SocketAddr, sync::Arc};

use log::debug;

use anyhow::Result;
use crate::tftp::{packet::{Error, Packet}, Handler};

pub enum ControlFlow {
    Continue(Packet),
    Closed(Option<Packet>),
}

enum TransferState {
    Uninitialized,
    Reading { filename: String, mode: String, block: u16 },
    Complete,
}

pub struct State<H: Handler> {
    addr: SocketAddr,
    state: TransferState,
    handler: Arc<H>,
}

impl<H: Handler + 'static> State<H> {
    pub fn new(addr: SocketAddr, handler: Arc<H>) -> Self {
        Self {
            addr,
            state: TransferState::Uninitialized,
            handler,
        }
    }

    pub async fn handle(&mut self, packet: Packet) -> ControlFlow {
        let result = match &mut self.state {
            TransferState::Uninitialized => {
                match packet {
                    Packet::Rrq { filename, mode } => {
                        handle_read_request(self.handler.as_ref(), filename, mode).await
                    },
                    _ => self.error(),
                }
            },
            TransferState::Reading { filename, mode , block} => {
                match packet {
                    Packet::Ack { block: acked_block } => handle_ack(self.handler.as_ref(), &filename, &mode, block, acked_block).await,
                    Packet::Error { code, message } => self.error(),
                    _ => self.error(),
                }
            },
            TransferState::Complete => {
                self.error()
            },
        };

        match result {
            Ok(response) => {
                if let Some(next_state) = response.next_state {
                    self.state = next_state;
                }
                response.response
            },
            Err(e) => {
                log::error!("TFTP: Error occured for {}: {:?}", self.addr, e);
                let packet = Packet::Error {
                    code: Error::Undefined,
                    message: String::from("internal error occured"),
                };
                ControlFlow::Closed(Some(packet))
            }
        }
    }

    fn error(&self) -> Result<HandleResponse> {
        debug!("TFTP: Returning error {:?} to {}", Error::IllegalOperation, self.addr);
        let packet = Packet::Error {
            code: Error::IllegalOperation,
            message: String::new(),
        };
        let response = HandleResponse {
            next_state: Some(TransferState::Complete),
            response: ControlFlow::Closed(Some(packet)),
        };
        Ok(response)
    }
}

struct HandleResponse {
    next_state: Option<TransferState>,
    response: ControlFlow,
}

async fn handle_read_request<H: Handler>(handler: &H, filename: String, mode: String) -> Result<HandleResponse> {
    handler.read(&filename).await?;
    let next_state = TransferState::Reading { filename: filename, mode: mode, block: 0 };
    let reply = Packet::Data { block: 0, data: Vec::new() };
    Ok(HandleResponse { next_state: Some(next_state), response: ControlFlow::Continue(reply) })
}

async fn handle_ack<H: Handler>(handler: &H, filename: &String, mode: &String, block: &mut u16, acked_block: u16) -> Result<HandleResponse> {
    *block = acked_block;
    handler.read(&filename).await?;
    let reply = Packet::Data { block: 0, data: Vec::new() };
    Ok(HandleResponse { next_state: None, response: ControlFlow::Continue(reply) })
}

fn error() -> ControlFlow {
    ControlFlow::Closed(Some(Packet::Error { code: Error::IllegalOperation, message: String::new() }))
}
