use crate::err::Result;
use crate::protocol::models::local_file::local_pull_file::LocalPullFileResponse;
use crate::protocol::models::peer::list_peers::ListPeersResponse;
use crate::protocol::models::task::list_tasks::ListTasksResponse;
use crate::protocol::protocol::Protocol;
use crate::protocol::token::Token;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ApiResponseKind {
    Error(String),
    ListPeers(ListPeersResponse),
    LocalPullFile(LocalPullFileResponse),
    ListTasks(ListTasksResponse),
}

#[derive(Debug, Clone)]
pub struct ApiResponseMessage {
    pub response: ApiResponseKind,
}

impl Protocol for ApiResponseMessage {
    fn serialize(&self) -> Vec<u8> {
        // Format: +API_RESPONSE, $<response-bytes>
        let resp_bytes = bincode::serialize(&self.response).unwrap_or_else(|_e| Vec::new());
        let tokens = vec![
            Token::Simple(String::from("API_RESPONSE")),
            Token::Data(bytes::Bytes::from(resp_bytes)),
        ];
        let mut out = Vec::new();
        for t in tokens {
            out.extend_from_slice(&t.to_bytes());
        }
        out
    }

    fn deserialize(bytes: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        use std::io;
        let tokens = Token::parse_all(bytes)?;
        if tokens.len() != 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected 2 tokens for ApiResponseMessage, got {}",
                    tokens.len()
                ),
            )
            .into());
        }
        Self::from_tokens(&tokens)
    }

    fn from_tokens(tokens: &[Token]) -> Result<Self>
    where
        Self: Sized,
    {
        use std::io;
        if tokens.len() != 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected 2 tokens for ApiResponseMessage, got {}",
                    tokens.len()
                ),
            )
            .into());
        }
        match &tokens[0] {
            Token::Simple(s) if s == "API_RESPONSE" => {}
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected leading Simple(\"API_RESPONSE\"), got {:?}", other),
                )
                .into());
            }
        }
        let response = match &tokens[1] {
            Token::Data(b) => match bincode::deserialize::<ApiResponseKind>(&b[..]) {
                Ok(v) => v,
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("bincode decode ApiResponseKind failed: {}", e),
                    )
                    .into());
                }
            },
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Data for response, got {:?}", other),
                )
                .into());
            }
        };
        Ok(ApiResponseMessage { response })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn concat_tokens(tokens: Vec<Token>) -> Vec<u8> {
        let mut out = Vec::new();
        for t in tokens {
            out.extend_from_slice(&t.to_bytes());
        }
        out
    }

    #[test]
    fn serialize_format_error() {
        let msg = ApiResponseMessage {
            response: ApiResponseKind::Error("oops".to_string()),
        };
        let bytes = msg.serialize();
        let tokens = Token::parse_all(&bytes).expect("parse tokens");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], Token::Simple(ref s) if s == "API_RESPONSE"));
        // Compare Data payload equals bincode of Error("oops")
        let expected = bincode::serialize(&ApiResponseKind::Error("oops".to_string())).unwrap();
        match &tokens[1] {
            Token::Data(b) => assert_eq!(&b[..], &expected[..]),
            _ => panic!("expected Data token for response"),
        }
    }

    #[test]
    fn roundtrip_list_peers() {
        let resp = ApiResponseMessage {
            response: ApiResponseKind::ListPeers(ListPeersResponse { peers: vec![] }),
        };
        let bytes = resp.serialize();
        let parsed = ApiResponseMessage::deserialize(&bytes).expect("deserialize");
        match parsed.response {
            ApiResponseKind::ListPeers(v) => assert!(v.peers.is_empty()),
            _ => panic!("expected LIST_PEERS variant"),
        }
    }

    #[test]
    fn deserialize_wrong_header() {
        let payload = bincode::serialize(&ApiResponseKind::Error("x".into())).unwrap();
        let bytes = concat_tokens(vec![
            Token::Simple("WRONG".into()),
            Token::Data(Bytes::from(payload)),
        ]);
        let res = ApiResponseMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(
            s.contains("expected leading Simple(\"API_RESPONSE\")"),
            "{s}"
        );
    }

    #[test]
    fn deserialize_invalid_payload() {
        let bytes = concat_tokens(vec![
            Token::Simple("API_RESPONSE".into()),
            Token::Data(Bytes::from_static(b"not-bincode")),
        ]);
        let res = ApiResponseMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(s.contains("bincode decode ApiResponseKind failed"), "{s}");
    }

    #[test]
    fn deserialize_unexpected_token_count() {
        let payload = bincode::serialize(&ApiResponseKind::Error("x".into())).unwrap();
        let mut bytes = concat_tokens(vec![
            Token::Simple("API_RESPONSE".into()),
            Token::Data(Bytes::from(payload)),
        ]);
        // Append an extra token
        bytes.extend_from_slice(&Token::Null.to_bytes());
        let res = ApiResponseMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(
            s.contains("expected 2 tokens for ApiResponseMessage"),
            "{s}"
        );
    }
}
