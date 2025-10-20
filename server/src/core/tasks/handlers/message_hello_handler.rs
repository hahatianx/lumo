use crate::core::PEER_TABLE;
use crate::core::tasks::{AsyncHandleable, NetworkHandleable};
use crate::core::topology::Peer;
use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER, get_msg_sender};
use crate::network::protocol::messages::HelloMessage;
use crate::network::protocol::messages::hello_message::HelloMode;
use api_model::protocol::protocol::Protocol;
use async_trait::async_trait;
use bytes::Bytes;
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::str::FromStr;

#[async_trait]
impl AsyncHandleable for HelloMessage {
    async fn handle(&mut self) -> Result<()> {
        LOGGER.debug(format!("HelloMessage: {:?}", self));
        update_peer_table(&self).await?;

        if self.mode.is_request_reply() {
            LOGGER.debug("Received a hello message requiring response.");
            let sender = get_msg_sender().await?;
            let resp = HelloMessage::from_env(HelloMode::empty())?;
            let sock_addr = SocketAddr::V4(SocketAddrV4::new(
                self.from_ip.parse().unwrap(),
                self.from_port,
            ));
            LOGGER.debug(format!("Response HelloMessage: {:?}", &resp));
            let b = Bytes::from(resp.serialize());
            sender.send(sock_addr, b).await?;
        }
        Ok(())
    }
}

impl NetworkHandleable for HelloMessage {
    fn should_ignore_by_sockaddr_peer(&self, peer: &SocketAddr) -> bool {
        if peer.ip().is_loopback() {
            return true;
        }
        if peer.ip() == ENV_VAR.get().unwrap().get_ip_addr() {
            return true;
        }
        false
    }
}

fn generate_peer_from_hello_message(msg: &HelloMessage) -> Result<Peer> {
    let ip_addr = IpAddr::from_str(msg.from_ip.as_str())?;
    let peer_is_leader = msg.mode.is_leader();
    Ok(Peer::new(
        msg.mac_addr.clone(),
        msg.from_name.clone(),
        ip_addr,
        peer_is_leader,
    ))
}

pub async fn update_peer_table(msg: &HelloMessage) -> Result<()> {
    let peer = generate_peer_from_hello_message(msg)?;
    PEER_TABLE.update_peer(peer).await
}
