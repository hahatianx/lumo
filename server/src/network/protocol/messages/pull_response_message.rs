use crate::err::Result;
use crate::global_var::ENV_VAR;
use crate::network::protocol::HandleableNetworkProtocol;
use crate::utilities::crypto::{from_encryption, to_encryption};
use api_model::protocol::protocol::Protocol;
use api_model::protocol::token::Token;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::time::{Duration, SystemTime};

type Nonce = u64;
type Challenge = u64;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum PullRejectionReason {
    FileOutdated = 400,
    FileInvalid = 401,
    AccessDenied = 403,
    FileNotFound = 404,
    InternalError = 500,
}

impl Debug for PullRejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PullRejectionReason::FileOutdated => write!(f, "400: FileOutdated"),
            PullRejectionReason::FileInvalid => write!(f, "400: FileInvalid"),
            PullRejectionReason::AccessDenied => write!(f, "403: AccessDenied"),
            PullRejectionReason::FileNotFound => write!(f, "404: FileNotFound"),
            PullRejectionReason::InternalError => write!(f, "500: InternalError"),
        }
    }
}

impl Display for PullRejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum PullDecision {
    Accept(Challenge, Nonce),
    Reject(Challenge, PullRejectionReason),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    from_ip: String,
    decision: PullDecision,
    timestamp: SystemTime,
}

impl PullResponse {
    pub fn new(from_ip: String, decision: PullDecision) -> Self {
        Self {
            from_ip,
            decision,
            timestamp: SystemTime::now(),
        }
    }

    pub fn get_from_ip(&self) -> &str {
        &self.from_ip
    }

    pub fn get_decision(&self) -> &PullDecision {
        &self.decision
    }

    pub fn get_timestamp(&self) -> &SystemTime {
        &self.timestamp
    }

    pub fn to_encryption(&self) -> Result<Vec<u8>> {
        to_encryption(self, || {
            let iv: [u8; 16] = rand::random();
            Ok(iv)
        })
    }

    pub fn from_encryption(ciphertext: Box<[u8]>) -> Result<Self> {
        from_encryption(ciphertext)
    }

    pub fn timestamp_valid(&self) -> bool {
        let now = SystemTime::now();
        let diff = now
            .duration_since(self.timestamp)
            .unwrap_or(Duration::from_secs(0));
        diff.as_secs() < ENV_VAR.get().unwrap().get_pull_task_validity_in_sec()
    }
}

pub struct PullResponseMessage {
    pub from_ip: String,
    pub response: Bytes,
}

impl PullResponseMessage {
    pub fn new(response: PullResponse) -> Result<Self> {
        if let Some(ev) = crate::global_var::ENV_VAR.get() {
            let from_ip = ev.get_ip_addr();
            let encrypted_response = Bytes::from(response.to_encryption()?);
            return Ok(Self {
                from_ip: from_ip.to_string(),
                response: encrypted_response,
            });
        }
        Err("Failed to generate pull response because env_var not found.".into())
    }

    pub fn get_response(&self) -> Result<PullResponse> {
        PullResponse::from_encryption(self.response.clone().to_vec().into_boxed_slice())
    }
}

// PullResponseMessage is for responses we send; no network handling on server side for now.
impl HandleableNetworkProtocol for PullResponseMessage {}

impl Protocol for PullResponseMessage {
    fn serialize(&self) -> Vec<u8> {
        // Format: +PULL_RESPONSE, +from_ip, $<response-bytes>
        let tokens = vec![
            Token::Simple(String::from("PULL_RESPONSE")),
            Token::Simple(self.from_ip.clone()),
            Token::Data(self.response.clone()),
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
        let tokens = Token::parse_all(bytes)?;
        Self::from_tokens(&tokens)
    }

    fn from_tokens(tokens: &[Token]) -> Result<Self>
    where
        Self: Sized,
    {
        use std::io;
        if tokens.len() != 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected 3 tokens for PullResponseMessage, got {}",
                    tokens.len()
                ),
            )
            .into());
        }
        match &tokens[0] {
            Token::Simple(s) if s == "PULL_RESPONSE" => {}
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "expected leading Simple(\"PULL_RESPONSE\"), got {:?}",
                        other
                    ),
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
        let response = match &tokens[2] {
            Token::Data(b) => b.clone(),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Data for response, got {:?}", other),
                )
                .into());
            }
        };
        Ok(PullResponseMessage { from_ip, response })
    }
}
