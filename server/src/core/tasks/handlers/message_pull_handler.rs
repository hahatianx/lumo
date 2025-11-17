use crate::core::tasks::AsyncHandleable;
use crate::core::tasks::NetworkHandleable;
use crate::core::tasks::handlers::IGNORE_SELF;
use crate::err::Result;
use crate::fs::{PullRequestResult, RejectionReason, start_pull_request};
use crate::global_var::{ENV_VAR, LOGGER, get_msg_sender};
use crate::network::protocol;
use crate::network::protocol::messages::PullMessage;
use crate::network::protocol::messages::PullResponse;
use crate::network::protocol::messages::pull_message::PullRequest;
use crate::network::protocol::messages::pull_response_message::PullDecision;
use crate::network::protocol::messages::pull_response_message::PullResponseMessage;
use api_model::protocol::protocol::Protocol;
use async_trait::async_trait;
use bytes::Bytes;

impl PullMessage {
    async fn start_pull_request(request: &PullRequest) -> PullDecision {
        // process request, and generate response
        match start_pull_request(request.get_path(), request.get_checksum().into()).await {
            Ok(result) => {
                match result {
                    PullRequestResult::Accept(nonce) => {
                        LOGGER.trace(format!("[PullRequest] Accepted pull request for file '{}', challenge {}, nonce {}",
                                             request.get_path(), request.get_challenge(), nonce));
                        PullDecision::Accept(request.get_challenge(), nonce)
                    }
                    PullRequestResult::Reject(reason) => {
                        LOGGER.trace(format!("[PullRequest] Rejected pull request for file '{}', challenge {}, reason {:?}",
                                             request.get_path(), request.get_challenge(), &reason));
                        match reason {
                            RejectionReason::FileChecksumMismatch => PullDecision::Reject(
                                request.get_challenge(),
                                protocol::messages::PullRejectionReason::FileOutdated,
                            ),
                            RejectionReason::PathNotFound => PullDecision::Reject(
                                request.get_challenge(),
                                protocol::messages::PullRejectionReason::FileNotFound,
                            ),
                            RejectionReason::PathNotFile => PullDecision::Reject(
                                request.get_challenge(),
                                protocol::messages::PullRejectionReason::FileInvalid,
                            ),
                            RejectionReason::SystemError => PullDecision::Reject(
                                request.get_challenge(),
                                protocol::messages::PullRejectionReason::InternalError,
                            ),
                        }
                    }
                }
            }
            Err(e) => {
                LOGGER.error(format!(
                    "Failed to start pull request, err: {:?}",
                    e.to_string()
                ));
                PullDecision::Reject(0, protocol::messages::PullRejectionReason::InternalError)
            }
        }
    }
}

#[async_trait]
impl AsyncHandleable for PullMessage {
    async fn handle(&mut self) -> Result<()> {
        LOGGER.debug(format!("PullMessage: {:?}", self));

        let decision = match self.validate_and_parse() {
            Ok(request) => PullMessage::start_pull_request(&request).await,
            Err(_) => {
                PullDecision::Reject(0, protocol::messages::PullRejectionReason::AccessDenied)
            }
        };

        if let PullDecision::Reject(0, _) = decision {
            // silently ignore invalid requests
            // This happens when the server is restarted, and the client sends a request before the server is ready.
            return Ok(());
        }

        let response = generate_response(decision);
        let reply_message = PullResponseMessage::new(response)?;
        let sender = get_msg_sender().await?;

        let sock_addr = format!("{}:{}", self.from_ip, ENV_VAR.get().unwrap().get_port())
            .parse::<std::net::SocketAddr>()?;
        sender
            .send(sock_addr, Bytes::from(reply_message.serialize()))
            .await?;

        Ok(())
    }
}

impl NetworkHandleable for PullMessage {
    fn should_ignore_by_sockaddr_peer(&self, peer: &std::net::SocketAddr) -> bool {
        IGNORE_SELF(peer)
    }
}

fn generate_response(pull_decision: PullDecision) -> PullResponse {
    let from_ip = ENV_VAR.get().unwrap().get_ip_addr();
    PullResponse::new(from_ip.to_string(), pull_decision)
}
