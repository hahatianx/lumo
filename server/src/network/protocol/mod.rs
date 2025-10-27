use crate::core::tasks::{AsyncHandleable, NetworkHandleable};
use crate::err::Result;
use crate::network::protocol::messages::hello_message::HelloMessage;
use api_model::protocol::message::api_request_message::ApiRequestMessage;
use api_model::protocol::protocol::Protocol;
use api_model::protocol::token::Token;
use bytes::Bytes;

mod consensus;
pub mod messages;
pub use consensus::CUR_LEADER;

pub trait HandleableNetworkProtocol:
    Protocol + NetworkHandleable + AsyncHandleable + Send + 'static
{
}

pub fn parse_message(bytes: &Bytes) -> Result<Box<dyn HandleableNetworkProtocol>> {
    let tokens = Token::parse_all(bytes)?;

    match tokens.get(0) {
        Some(head) => match head {
            Token::Simple(str) => match str.as_str() {
                "HELLO" => Ok(Box::new(HelloMessage::from_tokens(&tokens)?)),
                "API_REQUEST" => Ok(Box::new(ApiRequestMessage::from_tokens(&tokens)?)),
                _ => unimplemented!(),
            },
            _ => Err(String::from("Unable to parse message because tokens are malformed.").into()),
        },
        None => Err(String::from("Unable to parse message because no tokens found.").into()),
    }
}
