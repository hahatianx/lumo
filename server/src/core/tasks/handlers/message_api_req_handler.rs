use crate::constants::LOCAL_ADDR;
use crate::core::tasks::handlers::IGNORE_PEER;
use crate::core::tasks::{AsyncHandleable, NetworkHandleable};
use crate::err::Result;
use crate::global_var::get_msg_sender;
use crate::interface::handlers::run_handler;
use crate::network::protocol::HandleableNetworkProtocol;
use api_model::protocol::message::api_request_message::ApiRequestMessage;
use api_model::protocol::message::api_response_message::ApiResponseMessage;
use api_model::protocol::protocol::Protocol;
use async_trait::async_trait;
use bytes::Bytes;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;

#[async_trait]
impl AsyncHandleable for ApiRequestMessage {
    async fn handle(&mut self) -> Result<()> {
        let response = run_handler(&self.request).await?;

        let serialized_bytes = Bytes::from(ApiResponseMessage { response }.serialize());

        let sender = get_msg_sender().await?;
        sender
            .send(
                SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from_str(LOCAL_ADDR)?,
                    self.from_port,
                )),
                serialized_bytes,
            )
            .await?;

        Ok(())
    }
}

impl NetworkHandleable for ApiRequestMessage {
    // Only accept requests from local machine
    fn should_ignore_by_sockaddr_peer(&self, peer: &std::net::SocketAddr) -> bool {
        IGNORE_PEER(peer)
    }
}

impl HandleableNetworkProtocol for ApiRequestMessage {}
