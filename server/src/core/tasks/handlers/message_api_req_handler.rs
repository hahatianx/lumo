use crate::core::tasks::{AsyncHandleable, NetworkHandleable};
use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER, get_msg_sender};
use crate::interface::handlers::list_peers::list_peers;
use crate::interface::handlers::local_pull_file::local_pull_file;
use crate::network::protocol::HandleableNetworkProtocol;
use api_model::protocol::message::api_request_message::{ApiRequestKind, ApiRequestMessage};
use api_model::protocol::message::api_response_message::ApiResponseMessage;
use api_model::protocol::protocol::Protocol;
use async_trait::async_trait;
use bytes::Bytes;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;

async fn run_handler(api_request_kind: &ApiRequestKind) -> Result<Bytes> {
    let response = match api_request_kind {
        ApiRequestKind::ListPeers(req) => list_peers(req).await,
        ApiRequestKind::LocalPullFile(req) => local_pull_file(req).await,
        _ => return Err(format!("Handler for {:?} not found", api_request_kind).into()),
    };
    let vec = ApiResponseMessage { response }.serialize();
    Ok(Bytes::from(vec))
}

#[async_trait]
impl AsyncHandleable for ApiRequestMessage {
    async fn handle(&mut self) -> Result<()> {
        LOGGER.debug(format!("Received API request: {:?}", self).as_str());
        let response = run_handler(&self.request).await?;

        let sender = get_msg_sender().await?;
        sender
            .send(
                SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from_str(&self.from_ip)?,
                    self.from_port,
                )),
                response,
            )
            .await?;

        Ok(())
    }
}

impl NetworkHandleable for ApiRequestMessage {
    // Only accept requests from local machine
    fn should_ignore_by_sockaddr_peer(&self, peer: &std::net::SocketAddr) -> bool {
        if peer.ip().is_loopback() {
            return false;
        }
        if peer.ip() == ENV_VAR.get().unwrap().get_ip_addr() {
            return false;
        }
        true
    }
}

impl HandleableNetworkProtocol for ApiRequestMessage {}
