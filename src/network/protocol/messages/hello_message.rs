use crate::err::Result;
use crate::global_var::ENV_VAR;
use crate::network::protocol::HandleableProtocol;
use crate::network::protocol::protocol::Protocol;
use crate::network::protocol::token::Token;
use bitflags::bitflags;
use std::fmt::{Debug, Display, Formatter};

bitflags! {
    #[derive(Default)]
    pub struct HelloMode: u8 {
        const REQUEST_REPLY = 1 << 0;
        const LEADER = 1 << 1;
    }
}

impl Display for HelloMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s: Vec<String> = vec![];
        if self.contains(HelloMode::REQUEST_REPLY) {
            s.push(" REQUEST_REPLY ".to_string());
        }
        if self.contains(HelloMode::LEADER) {
            s.push(" LEADER ".to_string());
        }
        write!(f, "[{}]", s.join("|"))
    }
}

impl HelloMode {
    pub fn is_request_reply(self) -> bool {
        self.contains(HelloMode::REQUEST_REPLY)
    }
    pub fn is_leader(self) -> bool {
        self.contains(HelloMode::LEADER)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HelloMessage {
    pub from_ip: String,
    pub from_port: u16,
    pub from_name: String,
    pub mac_addr: String,
    // Bitflags in `mode`:
    // 0b01: request reply
    // 0b10: I am leader
    pub mode: HelloMode,
}

impl Debug for HelloMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HelloMessage {{ from_ip: {}, from_port: {}, from_name: {}, mac_addr: {}, mode: {} }}",
            self.from_ip, self.from_port, self.from_name, self.mac_addr, self.mode
        )
    }
}

impl HelloMessage {
    pub fn new(
        from_ip: String,
        from_port: u16,
        from_name: String,
        mac_addr: String,
        mode: HelloMode,
    ) -> Self {
        Self {
            from_ip,
            from_port,
            from_name,
            mac_addr,
            mode,
        }
    }

    pub fn from_env(mode: HelloMode) -> Result<Self> {
        if let Some(ev) = ENV_VAR.get() {
            let from_ip = ev.get_ip_addr();
            let from_port = ev.get_port();
            let from_name = ev.get_machine_name();
            let mac_addr = ev.get_mac_addr();
            return Ok(HelloMessage::new(
                from_ip.to_string(),
                from_port,
                from_name,
                mac_addr,
                mode,
            ));
        }
        Err("Fail to fetch env var".into())
    }
}

impl HandleableProtocol for HelloMessage {}
impl Protocol for HelloMessage {
    fn serialize(&self) -> Vec<u8> {
        let tokens = vec![
            Token::Simple(String::from("HELLO")),
            Token::Simple(self.from_ip.clone()),
            Token::Integer(self.from_port as u64),
            Token::Simple(self.from_name.clone()),
            Token::Simple(self.mac_addr.clone()),
            Token::Integer(self.mode.bits() as u64),
        ];
        // Concatenate token wire-format bytes
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
        // Take ownership of tokens to avoid unnecessary clones
        let mut it = tokens.into_iter();

        // Expect leading HELLO marker
        match it.next() {
            Some(Token::Simple(s)) if s == "HELLO" => {}
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected leading Simple(\"HELLO\") token, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no tokens found").into());
            }
        }

        let from_ip = match it.next() {
            Some(Token::Simple(s)) => s,
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for from_ip, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "missing from_ip token").into(),
                );
            }
        };

        let from_port = match it.next() {
            Some(Token::Integer(v)) => {
                if v > u16::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("port out of range: {}", v),
                    )
                    .into());
                }
                v as u16
            }
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Integer for from_port, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "missing from_port token",
                )
                .into());
            }
        };

        let from_name = match it.next() {
            Some(Token::Simple(s)) => s,
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for from_name, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "missing from_name token",
                )
                .into());
            }
        };

        // Parse mac_addr
        let mac_addr = match it.next() {
            Some(Token::Simple(s)) => s,
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for mac_addr, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "missing mac_addr token").into(),
                );
            }
        };

        // Parse mode (u8)
        let mode = match it.next() {
            Some(Token::Integer(v)) => {
                if v > u8::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("mode out of range: {}", v),
                    )
                    .into());
                }
                match HelloMode::from_bits(v as u8) {
                    Some(m) => m,
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid mode bits: {}", v),
                        )
                        .into());
                    }
                }
            }
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Integer for mode, got {:?}", other),
                )
                .into());
            }
            None => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "missing mode token").into(),
                );
            }
        };

        // Ensure there are no extra tokens
        if let Some(extra) = it.next() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected extra token: {:?}", extra),
            )
            .into());
        }

        Ok(HelloMessage {
            from_ip,
            from_port,
            from_name,
            mac_addr,
            mode,
        })
    }

    fn from_tokens(tokens: &[Token]) -> Result<Self>
    where
        Self: Sized,
    {
        use std::io;
        if tokens.len() != 6 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected 6 tokens for HelloMessage, got {}", tokens.len()),
            )
            .into());
        }
        // Validate leading HELLO token
        match &tokens[0] {
            Token::Simple(s) if s == "HELLO" => {}
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected leading Simple(\"HELLO\"), got {:?}", other),
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
        let from_name = match &tokens[3] {
            Token::Simple(s) => s.clone(),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for from_name, got {:?}", other),
                )
                .into());
            }
        };
        let mac_addr = match &tokens[4] {
            Token::Simple(s) => s.clone(),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Simple for mac_addr, got {:?}", other),
                )
                .into());
            }
        };
        let mode = match &tokens[5] {
            Token::Integer(v) => {
                if *v > u8::MAX as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("mode out of range: {}", v),
                    )
                    .into());
                }
                match HelloMode::from_bits(*v as u8) {
                    Some(m) => m,
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid mode bits: {}", v),
                        )
                        .into());
                    }
                }
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected Integer for mode, got {:?}", other),
                )
                .into());
            }
        };
        Ok(HelloMessage {
            from_ip,
            from_port,
            from_name,
            mac_addr,
            mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg() -> HelloMessage {
        HelloMessage::new(
            "192.168.1.10".to_string(),
            8080,
            "alice".to_string(),
            "aa:bb:cc:dd:ee:ff".to_string(),
            HelloMode::REQUEST_REPLY,
        )
    }

    #[test]
    fn serialize_produces_expected_tokens() -> crate::err::Result<()> {
        let m = msg();
        let bytes = m.serialize();
        let tokens = Token::parse_all(&bytes)?;
        assert_eq!(tokens.len(), 6);
        match &tokens[0] {
            Token::Simple(s) => assert_eq!(s, "HELLO"),
            other => panic!("expected HELLO header, got {:?}", other),
        }
        match &tokens[1] {
            Token::Simple(s) => assert_eq!(s, "192.168.1.10"),
            other => panic!("expected ip Simple, got {:?}", other),
        }
        match &tokens[2] {
            Token::Integer(v) => assert_eq!(*v, 8080),
            other => panic!("expected port Integer, got {:?}", other),
        }
        match &tokens[3] {
            Token::Simple(s) => assert_eq!(s, "alice"),
            other => panic!("expected name Simple, got {:?}", other),
        }
        match &tokens[4] {
            Token::Simple(s) => assert_eq!(s, "aa:bb:cc:dd:ee:ff"),
            other => panic!("expected mac Simple, got {:?}", other),
        }
        match &tokens[5] {
            Token::Integer(v) => assert_eq!(*v, HelloMode::REQUEST_REPLY.bits() as u64),
            other => panic!("expected mode Integer, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_serialize_deserialize() -> crate::err::Result<()> {
        let m = msg();
        let bytes = m.serialize();
        let back = HelloMessage::deserialize(&bytes)?;
        assert_eq!(m, back);
        Ok(())
    }

    #[test]
    fn from_tokens_rejects_wrong_header_and_length() {
        // wrong header
        let bad_header = vec![
            Token::Simple("H3LLO".into()),
            Token::Simple("192.168.1.10".into()),
            Token::Integer(8080),
            Token::Simple("alice".into()),
            Token::Simple("aa:bb:cc:dd:ee:ff".into()),
            Token::Integer(1),
        ];
        assert!(HelloMessage::from_tokens(&bad_header).is_err());

        // wrong length (missing mode)
        let too_short = vec![
            Token::Simple("HELLO".into()),
            Token::Simple("192.168.1.10".into()),
            Token::Integer(8080),
            Token::Simple("alice".into()),
            Token::Simple("aa:bb:cc:dd:ee:ff".into()),
        ];
        assert!(HelloMessage::from_tokens(&too_short).is_err());
    }

    #[test]
    fn from_tokens_rejects_out_of_range_mode() {
        let tokens = vec![
            Token::Simple("HELLO".into()),
            Token::Simple("192.168.1.10".into()),
            Token::Integer(8080),
            Token::Simple("alice".into()),
            Token::Simple("aa:bb:cc:dd:ee:ff".into()),
            Token::Integer((u8::MAX as u64) + 1),
        ];
        assert!(HelloMessage::from_tokens(&tokens).is_err());
    }

    #[test]
    fn deserialize_rejects_extra_tokens() -> crate::err::Result<()> {
        // Create valid hello bytes
        let mut bytes = msg().serialize();
        // Append one extra token
        let extra = Token::Simple("EXTRA".into()).to_bytes();
        bytes.extend_from_slice(&extra);
        // Should fail due to extra token
        assert!(HelloMessage::deserialize(&bytes).is_err());
        Ok(())
    }
}
