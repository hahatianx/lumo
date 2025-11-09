use std::fmt::{Debug, Display};

pub enum ClientError {
    ConnectionBindError(String, String),
    ConnectionTimeoutError(String, String),
    ConnectionReceiverError(String, String),

    ResponseParseError(String, String),

    InternalError(String, String),
}

impl Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ConnectionBindError(msg, _) => write!(f, "Connection bind error: {}", msg),
            ClientError::ConnectionTimeoutError(msg, _) => {
                write!(f, "Connection timeout error: {}", msg)
            }
            ClientError::ConnectionReceiverError(msg, _) => {
                write!(f, "Connection receiver error: {}", msg)
            }
            ClientError::ResponseParseError(msg, _) => write!(f, "Response parse error: {}", msg),
            ClientError::InternalError(msg, _) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl Debug for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ConnectionBindError(msg, trace) => {
                write!(f, "Connection bind error: {}\nTrace: {}", msg, trace)
            }
            ClientError::ConnectionTimeoutError(msg, trace) => {
                write!(f, "Connection timeout error: {}\nTrace: {}", msg, trace)
            }
            ClientError::ConnectionReceiverError(msg, trace) => {
                write!(f, "Connection receiver error: {}\nTrace: {}", msg, trace)
            }
            ClientError::ResponseParseError(msg, trace) => {
                write!(f, "Response parse error: {}\nTrace: {}", msg, trace)
            }
            ClientError::InternalError(msg, trace) => {
                write!(f, "Internal error: {}\nTrace: {}", msg, trace)
            }
        }
    }
}
