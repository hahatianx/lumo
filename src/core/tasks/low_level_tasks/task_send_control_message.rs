use crate::core::tasks::AsyncHandleable;
use crate::err::Result;
use crate::global_var::{GLOBAL_VAR, GlobalVar, LOGGER};
use async_trait::async_trait;
use bytes::Bytes;
use std::net::SocketAddr;

pub enum SendType {
    Broadcast,
    Unicast(SocketAddr),
}

pub struct SendControlMessageTask {
    send_type: SendType,
    bytes: Bytes,
}

impl SendControlMessageTask {
    pub fn new(send_type: SendType, bytes: Bytes) -> Self {
        Self { send_type, bytes }
    }
}

#[async_trait]
impl AsyncHandleable for SendControlMessageTask {
    async fn handle(&mut self) -> Result<()> {
        let udp_sender = GLOBAL_VAR
            .get()
            .unwrap()
            .network_setup
            .lock()
            .await
            .as_ref()
            .unwrap()
            .sender
            .sender();

        // Move ownership of the payload out of self by replacing it with an empty Bytes.
        let bytes = std::mem::take(&mut self.bytes);

        match &self.send_type {
            SendType::Broadcast => {
                udp_sender.broadcast(bytes).await?;
            }
            SendType::Unicast(addr) => {
                udp_sender.send(*addr, bytes).await?;
            }
        }

        Ok(())
    }
}

impl SendControlMessageTask {
    #[cfg(test)]
    fn new_unicast(addr: SocketAddr, bytes: Bytes) -> Self {
        Self {
            send_type: SendType::Unicast(addr),
            bytes,
        }
    }

    #[cfg(test)]
    fn new_broadcast(bytes: Bytes) -> Self {
        Self {
            send_type: SendType::Broadcast,
            bytes,
        }
    }

    #[cfg(test)]
    fn drain_bytes_for_test(&mut self) -> Bytes {
        std::mem::take(&mut self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn drain_bytes_moves_payload_for_unicast() {
        let addr: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut task = SendControlMessageTask::new_unicast(addr, Bytes::from_static(b"payload"));
        let drained = task.drain_bytes_for_test();
        assert_eq!(&drained[..], b"payload");
        assert!(task.bytes.is_empty());
    }

    #[test]
    fn drain_bytes_moves_payload_for_broadcast() {
        let mut task = SendControlMessageTask::new_broadcast(Bytes::from_static(b"bc"));
        let drained = task.drain_bytes_for_test();
        assert_eq!(&drained[..], b"bc");
        assert!(task.bytes.is_empty());
    }
}
