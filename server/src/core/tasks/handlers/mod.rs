mod message_api_req_handler;
mod message_hello_handler;
mod message_pull_handler;

use async_trait::async_trait;
use std::net::SocketAddr;

use crate::err::Result;
use crate::global_var::ENV_VAR;

pub static IGNORE_SELF: fn(&SocketAddr) -> bool = |peer: &SocketAddr| {
    if peer.ip().is_loopback() {
        return true;
    }
    if peer.ip() == ENV_VAR.get().unwrap().get_ip_addr() {
        return true;
    }
    false
};

pub static IGNORE_PEER: fn(&SocketAddr) -> bool = |peer: &SocketAddr| !IGNORE_SELF(peer);

pub trait NetworkHandleable {
    fn should_ignore_by_sockaddr_peer(&self, peer: &SocketAddr) -> bool;
}

#[async_trait]
pub trait AsyncHandleable: Send + 'static {
    async fn handle(&mut self) -> Result<()>;
}
