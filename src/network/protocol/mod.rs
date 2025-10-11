use crate::err::Result;
use crate::tasks::Handleable;
use bytes::Bytes;
use crate::network::protocol::messages::hello_message::HelloMessage;
use crate::network::protocol::protocol::Protocol;
use crate::network::protocol::token::Token;

pub mod messages;
pub mod protocol;
pub mod token;

pub trait HandleableProtocol: protocol::Protocol + Handleable + Send + 'static {}

pub fn parse_message(bytes: &Bytes) -> Result<Box<dyn HandleableProtocol>> {
    let tokens = Token::parse_all(bytes)?;

    match tokens.get(0) {
        Some(head) => {
            match head {
                Token::Simple(str) => {
                    match str.as_str() {
                        "HELLO" => Ok(Box::new(HelloMessage::from_tokens(&tokens)?)),
                        _ => unimplemented!(),
                    }
                },
                _ => {
                    Err(String::from("Unable to parse message because tokens are malformed.").into())
                }
            }
        },
        None => {
            Err(String::from("Unable to parse message because no tokens found.").into())
        }
    }
}
