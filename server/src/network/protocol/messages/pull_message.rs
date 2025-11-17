use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::protocol::HandleableNetworkProtocol;
use crate::utilities::crypto::from_encryption;
use crate::utilities::crypto::to_encryption;
use api_model::protocol::protocol::Protocol;
use api_model::protocol::token::Token;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Debug;
use std::time::{Duration, SystemTime};

type Challenge = u64;
type Checksum = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    from_ip: String,

    path: String,
    checksum: Option<Checksum>,

    challenge: u64,
    time_stamp: SystemTime,
}

impl PullRequest {
    pub fn new<T>(from_ip: String, path: String, checksum: T, challenge: u64) -> Self
    where
        T: Into<Option<Checksum>>,
    {
        Self {
            from_ip,
            path,
            checksum: checksum.into(),
            challenge,
            time_stamp: SystemTime::now(),
        }
    }

    pub fn get_from_ip(&self) -> &str {
        &self.from_ip
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }

    pub fn get_challenge(&self) -> u64 {
        self.challenge
    }

    pub fn get_checksum(&self) -> Option<Checksum> {
        self.checksum
    }

    pub fn request_time_valid(&self) -> bool {
        let now = SystemTime::now();
        let diff = now.duration_since(self.time_stamp).unwrap_or(Duration::from_secs(0));
        diff.as_secs() < ENV_VAR.get().unwrap().get_pull_task_validity_in_sec()
    }

    fn generate_iv_from_challenge(challenge: Challenge) -> Result<[u8; 16]> {
        let mut hasher = Sha256::new();
        hasher.update(challenge.to_be_bytes());
        hasher.update(b"pull_iv");
        let digest = hasher.finalize();
        let iv: [u8; 16] = digest[..16].try_into().map_err(|_| "IV too short")?;
        Ok(iv)
    }

    pub fn to_encryption(&self) -> Result<Vec<u8>> {
        to_encryption(self, || Self::generate_iv_from_challenge(self.challenge))
    }

    pub fn from_encryption(ciphertext: Box<[u8]>) -> Result<Self> {
        from_encryption(ciphertext)
    }
}

pub struct PullMessage {
    pub from_ip: String,
    pub request: Bytes,
}

impl Debug for PullMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PullMessage {{ from_ip: {}, request: <encrypted> }}",
            self.from_ip
        )?;
        match PullRequest::from_encryption(self.request.clone().to_vec().into_boxed_slice()) {
            Ok(request) => write!(
                f,
                "PullRequest {{ path: {}, checksum: {:?}, challenge: {} }}",
                request.path, request.checksum, request.challenge
            ),
            Err(_) => write!(f, "PullRequest {{ <decryption failed> }}"),
        }
    }
}

impl PullMessage {
    pub fn new<T>(path: &str, checksum: T, challenge: u64) -> Result<Self>
    where
        T: Into<Option<Checksum>>,
    {
        if let Some(ev) = ENV_VAR.get() {
            let from_ip = ev.get_ip_addr();

            let encrypted_request =
                PullRequest::new(from_ip.to_string(), path.to_string(), checksum, challenge)
                    .to_encryption()?;

            return Ok(Self {
                from_ip: from_ip.to_string(),
                request: encrypted_request.into(),
            });
        }

        Err("Failed to generate pull message because env_var not found.".into())
    }

    pub fn validate_and_parse(&self) -> Result<PullRequest> {
        let from_ip_out = &self.from_ip;

        let normalized_data = self.request.to_vec().into_boxed_slice();

        match PullRequest::from_encryption(normalized_data) {
            Ok(pull_request) => {
                if !pull_request.request_time_valid() {
                    let time_diff = SystemTime::now()
                        .duration_since(pull_request.time_stamp)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs();
                    LOGGER.warn(format!(
                        "Pull request from {} is too old. The request was generated {} seconds ago",
                        &from_ip_out, time_diff
                    ));
                    return Err("Request is too old".into());
                }
                if from_ip_out != pull_request.get_from_ip() {
                    LOGGER.warn(format!(
                        "Pull request from {} is not from the same IP as the server",
                        &from_ip_out
                    ));
                    return Err("Request is not from the same IP".into());
                }
                Ok(pull_request)
            }
            Err(e) => {
                LOGGER.warn(format!("Failed to deserialize pull request: {}", e));
                Err("Request decryption failed".into())
            }
        }
    }
}

impl HandleableNetworkProtocol for PullMessage {}

impl Protocol for PullMessage {
    fn serialize(&self) -> Vec<u8> {
        // Format: +PULL, +from_ip, $<request-bytes>
        let tokens = vec![
            Token::Simple(String::from("PULL")),
            Token::Simple(self.from_ip.clone()),
            Token::Data(self.request.clone()),
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
                format!("expected 3 tokens for PullMessage, got {}", tokens.len()),
            )
            .into());
        }
        match &tokens[0] {
            Token::Simple(s) if s == "PULL" => {}
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected leading Simple(\"PULL\"), got {:?}", other),
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
        let request = match &tokens[2] {
            Token::Data(b) => b.clone(),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Data for request, got {:?}", other),
                )
                .into());
            }
        };
        Ok(PullMessage { from_ip, request })
    }
}
