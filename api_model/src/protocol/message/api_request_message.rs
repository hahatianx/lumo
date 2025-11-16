use crate::err::Result;
use crate::protocol::models::file::pull_file::PullFileRequest;
use crate::protocol::models::local_file::local_pull_file::LocalPullFileRequest;
use crate::protocol::models::peer::list_peers::ListPeersRequest;
use crate::protocol::models::task::list_tasks::ListTasksRequest;
use crate::protocol::protocol::Protocol;
use crate::protocol::token::Token;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ApiRequestKind {
    Info,
    ListPeers(ListPeersRequest),
    LocalPullFile(LocalPullFileRequest),
    PullFile(PullFileRequest),
    ListTasks(ListTasksRequest),
}

#[derive(Debug, Clone)]
pub struct ApiRequestMessage {
    pub from_ip: String,
    pub from_port: u16,
    pub request: ApiRequestKind,
}

impl ApiRequestMessage {
    pub fn new(from_ip: String, from_port: u16, request: ApiRequestKind) -> Self {
        Self {
            from_ip,
            from_port,
            request,
        }
    }
}

impl Protocol for ApiRequestMessage {
    fn serialize(&self) -> Vec<u8> {
        // Format: +API_REQUEST, +from_ip, :from_port, $<request-bytes>
        let request_bytes = bincode::serialize(&self.request).unwrap_or_else(|_e| Vec::new());
        let tokens = vec![
            Token::Simple(String::from("API_REQUEST")),
            Token::Simple(self.from_ip.clone()),
            Token::Integer(self.from_port as u64),
            Token::Data(bytes::Bytes::from(request_bytes)),
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
        // We expect exactly 4 tokens
        if tokens.len() != 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected 4 tokens for ApiRequestMessage, got {}",
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
        if tokens.len() != 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected 4 tokens for ApiRequestMessage, got {}",
                    tokens.len()
                ),
            )
            .into());
        }
        match &tokens[0] {
            Token::Simple(s) if s == "API_REQUEST" => {}
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected leading Simple(\"API_REQUEST\"), got {:?}", other),
                )
                .into());
            }
        }
        let from_ip = match &tokens[1] {
            Token::Simple(s) => s.clone(),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for from_ip, got {:?}", other),
                )
                .into());
            }
        };
        let from_port = match &tokens[2] {
            Token::Integer(v) => {
                if *v > u16::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("port out of range: {}", v),
                    )
                    .into());
                }
                *v as u16
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Integer for from_port, got {:?}", other),
                )
                .into());
            }
        };
        let request = match &tokens[3] {
            Token::Data(b) => match bincode::deserialize::<ApiRequestKind>(&b[..]) {
                Ok(v) => v,
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("bincode decode ApiRequestKind failed: {}", e),
                    )
                    .into());
                }
            },
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Data for request, got {:?}", other),
                )
                .into());
            }
        };
        Ok(ApiRequestMessage {
            from_ip,
            from_port,
            request,
        })
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
    fn serialize_format_info() {
        let msg = ApiRequestMessage {
            from_ip: "127.0.0.1".to_string(),
            from_port: 8080,
            request: ApiRequestKind::Info,
        };
        let bytes = msg.serialize();
        let tokens = Token::parse_all(&bytes).expect("parse tokens");
        assert_eq!(tokens.len(), 4);
        assert!(matches!(tokens[0], Token::Simple(ref s) if s == "API_REQUEST"));
        assert!(matches!(tokens[1], Token::Simple(ref s) if s == "127.0.0.1"));
        assert!(matches!(tokens[2], Token::Integer(8080)));
        // Compare Data payload equals bincode of INFO
        let expected = bincode::serialize(&ApiRequestKind::Info).unwrap();
        match &tokens[3] {
            Token::Data(b) => assert_eq!(&b[..], &expected[..]),
            _ => panic!("expected Data token for request"),
        }
    }

    #[test]
    fn roundtrip_list_peers() {
        let msg = ApiRequestMessage {
            from_ip: "10.0.0.2".to_string(),
            from_port: 6553,
            request: ApiRequestKind::ListPeers(ListPeersRequest),
        };
        let bytes = msg.serialize();
        let parsed = ApiRequestMessage::deserialize(&bytes).expect("deserialize");
        assert_eq!(parsed.from_ip, "10.0.0.2");
        assert_eq!(parsed.from_port, 6553);
        match parsed.request {
            ApiRequestKind::ListPeers(_) => {}
            _ => panic!("expected LIST_PEERS variant"),
        }
    }

    #[test]
    fn deserialize_wrong_header() {
        let payload = bincode::serialize(&ApiRequestKind::Info).unwrap();
        let bytes = concat_tokens(vec![
            Token::Simple("WRONG".into()),
            Token::Simple("1.2.3.4".into()),
            Token::Integer(1234),
            Token::Data(Bytes::from(payload)),
        ]);
        let res = ApiRequestMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(
            s.contains("expected leading Simple(\"API_REQUEST\")"),
            "{s}"
        );
    }

    #[test]
    fn deserialize_wrong_from_ip_type() {
        let payload = bincode::serialize(&ApiRequestKind::Info).unwrap();
        let bytes = concat_tokens(vec![
            Token::Simple("API_REQUEST".into()),
            Token::Integer(1), // wrong type
            Token::Integer(1234),
            Token::Data(Bytes::from(payload)),
        ]);
        let res = ApiRequestMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(s.contains("expected Simple for from_ip"), "{s}");
    }

    #[test]
    fn deserialize_port_out_of_range() {
        let payload = bincode::serialize(&ApiRequestKind::Info).unwrap();
        let bytes = concat_tokens(vec![
            Token::Simple("API_REQUEST".into()),
            Token::Simple("host".into()),
            Token::Integer(u16::MAX as u64 + 1),
            Token::Data(Bytes::from(payload)),
        ]);
        let res = ApiRequestMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(s.contains("port out of range"), "{s}");
    }

    #[test]
    fn deserialize_invalid_request_payload() {
        let bytes = concat_tokens(vec![
            Token::Simple("API_REQUEST".into()),
            Token::Simple("host".into()),
            Token::Integer(1),
            Token::Data(Bytes::from_static(b"not-bincode")),
        ]);
        let res = ApiRequestMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(s.contains("bincode decode ApiRequestKind failed"), "{s}");
    }

    #[test]
    fn deserialize_unexpected_token_count() {
        let payload = bincode::serialize(&ApiRequestKind::Info).unwrap();
        let mut bytes = concat_tokens(vec![
            Token::Simple("API_REQUEST".into()),
            Token::Simple("host".into()),
            Token::Integer(1),
            Token::Data(Bytes::from(payload)),
        ]);
        // Append an extra token
        bytes.extend_from_slice(&Token::Null.to_bytes());
        let res = ApiRequestMessage::deserialize(&bytes);
        assert!(res.is_err());
        let s = res.err().unwrap().to_string();
        assert!(s.contains("expected 4 tokens for ApiRequestMessage"), "{s}");
    }
}
