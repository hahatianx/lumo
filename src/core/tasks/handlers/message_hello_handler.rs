use crate::core::PEER_TABLE;
use crate::core::tasks::AsyncHandleable;
use crate::core::topology::Peer;
use crate::err::Result;
use crate::global_var::{LOGGER, get_msg_sender};
use crate::network::protocol::messages::HelloMessage;
use crate::network::protocol::protocol::Protocol;
use async_trait::async_trait;
use bytes::Bytes;
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::str::FromStr;

#[async_trait]
impl AsyncHandleable for HelloMessage {
    async fn handle(&mut self) -> Result<()> {
        LOGGER.debug(format!("HelloMessage: {:?}", self));
        update_peer_table(&self).await?;

        if self.mode == 1 {
            let sender = get_msg_sender().await?;
            let resp = HelloMessage::from_env()?;
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

fn generate_peer_from_hello_message(msg: &HelloMessage) -> Result<Peer> {
    let ip_addr = IpAddr::from_str(msg.from_ip.as_str())?;
    Ok(Peer::new(
        msg.mac_addr.clone(),
        msg.from_name.clone(),
        ip_addr,
        false,
    ))
}

pub async fn update_peer_table(msg: &HelloMessage) -> Result<()> {
    let peer = generate_peer_from_hello_message(msg)?;
    PEER_TABLE.update_peer(peer).await
}
