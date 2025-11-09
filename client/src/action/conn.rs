use crate::error::ClientError;
use api_model::protocol::message::api_request_message::{ApiRequestKind, ApiRequestMessage};
use api_model::protocol::message::api_response_message::{ApiResponseKind, ApiResponseMessage};
use api_model::protocol::protocol::Protocol;
use std::net::UdpSocket;

pub struct ConnectionConfig {
    api_port: u16,
    size_in_kb: u32,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            api_port: 14514,
            size_in_kb: 1024,
        }
    }
}

pub struct Connection {
    udp_socket: UdpSocket,
    config: ConnectionConfig,
}

impl Connection {
    pub fn new(config: Option<ConnectionConfig>) -> Result<Self, ClientError> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| {
            ClientError::ConnectionBindError(
                String::from("failed to create UDP socket"),
                e.to_string(),
            )
        })?;

        socket
            .set_nonblocking(false)
            .map_err(|e| ClientError::ConnectionBindError(String::from(""), e.to_string()))?;

        match config {
            Some(c) => Ok(Self {
                udp_socket: socket,
                config: c,
            }),
            None => Ok(Self {
                udp_socket: socket,
                config: ConnectionConfig::default(),
            }),
        }
    }

    fn serialize_payload(&self, api_request: ApiRequestKind) -> Result<Vec<u8>, ClientError> {
        let local_addr = self.udp_socket.local_addr().map_err(|e| {
            ClientError::ConnectionBindError(
                String::from("failed to get local addr"),
                e.to_string(),
            )
        })?;

        let payload =
            ApiRequestMessage::new(local_addr.ip().to_string(), local_addr.port(), api_request)
                .serialize();

        Ok(payload)
    }

    fn receive_response(&self) -> Result<ApiResponseMessage, ClientError> {
        let sz: usize = (self.config.size_in_kb * 1024 + 5) as usize;
        let mut buf: Vec<u8> = vec![0; sz];

        let response = self.udp_socket.recv_from(&mut buf).map_err(|e| {
            ClientError::ConnectionReceiverError(
                String::from("failed to receive response"),
                e.to_string(),
            )
        })?;

        if response.0 > (self.config.size_in_kb * 1024) as usize {
            return Err(ClientError::ResponseParseError(
                String::from("response size exceeds limit"),
                String::from(""),
            ));
        }

        Ok(
            ApiResponseMessage::deserialize(&buf[..response.0]).map_err(|e| {
                ClientError::ResponseParseError(
                    String::from("failed to deserialize response"),
                    e.to_string(),
                )
            })?,
        )
    }

    pub fn request(&self, api_request: ApiRequestKind) -> Result<ApiResponseKind, ClientError> {
        let payload = self.serialize_payload(api_request)?;
        let addr = format!("{}:{}", "127.0.0.1", self.config.api_port);

        self.udp_socket
            .send_to(payload.as_ref(), addr)
            .map_err(|e| {
                ClientError::ConnectionReceiverError(
                    String::from("failed to send request"),
                    e.to_string(),
                )
            })?;

        let response = self.receive_response()?;
        Ok(response.response)
    }
}
