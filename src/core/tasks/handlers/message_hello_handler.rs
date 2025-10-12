use std::net::{SocketAddr, SocketAddrV4};
use crate::core::tasks::AsyncHandleable;
use crate::core::tasks::helpers::update_peer_table;
use crate::err::Result;
use crate::global_var::{get_msg_sender, LOGGER};
use crate::network::protocol::messages::HelloMessage;
use async_trait::async_trait;
use bytes::Bytes;
use crate::network::protocol::protocol::Protocol;

#[async_trait]
impl AsyncHandleable for HelloMessage {
    async fn handle(&mut self) -> Result<()> {
        LOGGER.debug(format!("HelloMessage: {:?}", self));
        update_peer_table(&self).await?;

        if self.mode == 1 {
            let sender = get_msg_sender().await?;
            let resp = HelloMessage::from_env()?;
            let sock_addr = SocketAddr::V4(SocketAddrV4::new(self.from_ip.parse().unwrap(), self.from_port));
            LOGGER.debug(format!("Response HelloMessage: {:?}", &resp));
            let b = Bytes::from(resp.serialize());
            sender.send(sock_addr, b).await?;
        }
        Ok(())
    }
}
