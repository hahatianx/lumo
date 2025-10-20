mod message_api_req_handler;
mod message_hello_handler;

use async_trait::async_trait;
use std::net::SocketAddr;

use crate::err::Result;

pub trait NetworkHandleable {
    fn should_ignore_by_sockaddr_peer(&self, peer: &SocketAddr) -> bool;
}

#[async_trait]
pub trait AsyncHandleable: Send + 'static {
    async fn handle(&mut self) -> Result<()>;
}
