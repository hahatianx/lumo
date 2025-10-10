use crate::err::Result;

use super::token::Token;

pub trait Protocol {
    /// Serialize this value into a vector of bytes.
    fn serialize(&self) -> Vec<u8>;

    /// Construct an instance from a slice of bytes.
    fn deserialize(bytes: &[u8]) -> Result<Self>
    where
        Self: Sized;

    /// Construct an instance from a sequence of tokens.
    fn from_tokens(tokens: &[Token]) -> Result<Self>
    where
        Self: Sized;
}
