use crate::err::Result;
use bytes::Bytes;
use std::io;

#[derive(Clone, Debug)]
pub enum Token {
    // +XXXX\r\n
    Simple(String),
    // -XXXX\r\n
    Error(String),
    // $XXXX\r\n
    Data(Bytes),
    // :XXXX\r\n
    Integer(u64),
    // ,XXXX\r\n
    Float(f64),
    // ^\r\n
    Null,
}

impl Token {
    #[inline]
    fn to_utf8(bytes: &[u8]) -> Result<&str> {
        std::str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e).into())
    }

    #[inline]
    fn to_string(bytes: &[u8]) -> Result<String> {
        String::from_utf8(bytes.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e).into())
    }

    #[inline]
    fn parse_from_str<T>(s: &str, what: &str) -> Result<T>
    where
        T: std::str::FromStr,
        <T as std::str::FromStr>::Err: std::fmt::Display,
    {
        s.parse::<T>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid {}: {}", what, e),
            )
            .into()
        })
    }

    /// Convert this token to its wire-format bytes.
    /// Formats mirror the parser:
    /// - +<utf8>\r\n for Simple
    /// - -<utf8>\r\n for Error
    /// - $<bytes>\r\n for Data
    /// - :<u64>\r\n for Integer
    /// - ,<f64>\r\n for Float
    /// - ^\r\n       for Null
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Token::Simple(s) => {
                out.push(b'+');
                out.extend_from_slice(s.as_bytes());
            }
            Token::Error(s) => {
                out.push(b'-');
                out.extend_from_slice(s.as_bytes());
            }
            Token::Data(b) => {
                out.push(b'$');
                out.extend_from_slice(b);
            }
            Token::Integer(v) => {
                out.push(b':');
                let mut buf = [0u8; lexical_core::BUFFER_SIZE];
                let slc = lexical_core::write(*v, &mut buf);
                out.extend_from_slice(slc);
            }
            Token::Float(v) => {
                out.push(b',');
                let mut buf = [0u8; lexical_core::BUFFER_SIZE];
                let slc = lexical_core::write(*v, &mut buf);
                out.extend_from_slice(slc);
            }
            Token::Null => {
                out.push(b'^');
            }
        }
        out.extend_from_slice(b"\r\n");
        out
    }

    /// Parse a single token from the given byte slice.
    /// Returns the parsed token and the number of bytes consumed.
    ///
    /// Formats:
    /// - +<utf8>\r\n => Simple
    /// - -<utf8>\r\n => Error
    /// - $<bytes>\r\n => Data (raw bytes between marker and CRLF)
    /// - :<u64>\r\n => Integer
    /// - ,<f64>\r\n => Float
    /// - ^\r\n       => Null
    pub fn parse_one(input: &[u8]) -> Result<(Token, usize)> {
        if input.is_empty() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty input").into());
        }
        // Find CRLF
        let mut i = 0usize;
        let mut crlf_pos: Option<usize> = None;
        while i + 1 < input.len() {
            if input[i] == b'\r' && input[i + 1] == b'\n' {
                crlf_pos = Some(i);
                break;
            }
            i += 1;
        }
        let end =
            crlf_pos.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing CRLF"))?;
        if input[0] == b'^' {
            // Expect exactly '^\r\n'
            if end != 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "null token must be exactly '^\\r\\n'",
                )
                .into());
            }
            return Ok((Token::Null, end + 2));
        }
        if input.len() < 2 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "too short").into());
        }
        let (prefix, body) = (input[0], &input[1..end]);
        let consumed = end + 2;
        let token = match prefix {
            b'+' => Token::Simple(Self::to_string(body)?),
            b'-' => Token::Error(Self::to_string(body)?),
            b'$' => Token::Data(Bytes::copy_from_slice(body)),
            b':' => match lexical_core::parse::<u64>(body) {
                Ok(v) => Token::Integer(v),
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid integer: {:?}", e),
                    )
                    .into());
                }
            },
            b',' => match lexical_core::parse::<f64>(body) {
                Ok(v) => Token::Float(v),
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid float: {:?}", e),
                    )
                    .into());
                }
            },
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown token prefix: {}", other as char),
                )
                .into());
            }
        };
        Ok((token, consumed))
    }

    /// Parse all tokens from the input until exhaustion.
    /// Uses an index cursor without modifying the input slice.
    pub fn parse_all(input: &[u8]) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut idx: usize = 0;
        while idx < input.len() {
            let (tok, used) = Self::parse_one(&input[idx..])?;
            tokens.push(tok);
            idx += used;
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let (t, used) = Token::parse_one(b"+OK\r\n").unwrap();
        assert!(matches!(t, Token::Simple(ref s) if s == "OK"));
        assert_eq!(used, 5);
    }

    #[test]
    fn parse_error() {
        let (t, _used) = Token::parse_one(b"-ERR something\r\n").unwrap();
        assert!(matches!(t, Token::Error(ref s) if s == "ERR something"));
    }

    #[test]
    fn parse_data() {
        let (t, _used) = Token::parse_one(b"$abc\r\n").unwrap();
        match t {
            Token::Data(b) => assert_eq!(&b[..], b"abc"),
            _ => panic!("wrong token"),
        }
    }

    #[test]
    fn parse_integer() {
        let (t, _used) = Token::parse_one(b":42\r\n").unwrap();
        assert!(matches!(t, Token::Integer(42)));
    }

    #[test]
    fn parse_float() {
        let (t, _used) = Token::parse_one(b",3.14\r\n").unwrap();
        if let Token::Float(v) = t {
            assert!((v - 3.14).abs() < 1e-10);
        } else {
            panic!("wrong token");
        }
    }

    #[test]
    fn parse_null() {
        let (t, _used) = Token::parse_one(b"^\r\n").unwrap();
        assert!(matches!(t, Token::Null));
    }

    #[test]
    fn parse_all_sequence() {
        let tokens = Token::parse_all(b"+OK\r\n:1\r\n^\r\n").unwrap();
        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn to_bytes_simple() {
        let t = Token::Simple("OK".into());
        assert_eq!(&t.to_bytes()[..], b"+OK\r\n");
    }

    #[test]
    fn to_bytes_error() {
        let t = Token::Error("ERR something".into());
        assert_eq!(&t.to_bytes()[..], b"-ERR something\r\n");
    }

    #[test]
    fn to_bytes_data() {
        let t = Token::Data(Bytes::from_static(b"abc"));
        assert_eq!(&t.to_bytes()[..], b"$abc\r\n");
    }

    #[test]
    fn to_bytes_integer() {
        let t = Token::Integer(42);
        assert_eq!(&t.to_bytes()[..], b":42\r\n");
    }

    #[test]
    fn to_bytes_float() {
        let t = Token::Float(3.14);
        let s = t.to_bytes();
        assert!(std::str::from_utf8(&s).unwrap().starts_with(",3.14"));
        assert!(s.ends_with(b"\r\n"));
    }

    #[test]
    fn to_bytes_null() {
        let t = Token::Null;
        assert_eq!(&t.to_bytes()[..], b"^\r\n");
    }

    #[test]
    fn round_trip() {
        let seq = vec![
            Token::Simple("OK".into()),
            Token::Integer(1),
            Token::Null,
            Token::Error("NO".into()),
            Token::Data(Bytes::from_static(b"xy")),
            Token::Float(2.5),
        ];
        let mut bytes = Vec::new();
        for t in &seq {
            bytes.extend_from_slice(&t.to_bytes());
        }
        let parsed = Token::parse_all(&bytes).unwrap();
        assert_eq!(parsed.len(), seq.len());
    }
}
