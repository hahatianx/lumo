use crate::constants::UPD_MESSAGE_PORT;
use crate::err::Result;
use crate::global_var::LOGGER;
use bytes::Bytes;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// UdpListener binds to an address and continuously listens for UDP datagrams.
/// It can bind from ENV_VAR or from an explicit SocketAddr (useful for tests).
pub struct UdpListener {
    socket: UdpSocket,
}

/// Handle to a running UDP listener task, allowing graceful shutdown.
#[derive(Debug)]
pub struct ListenerHandle {
    handle: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
}

impl ListenerHandle {
    /// Signal shutdown and await the listener task to exit.
    pub async fn shutdown(self) -> Result<()> {
        // Ignore if already closed
        let _ = self.shutdown_tx.send(());
        let _ = self.handle.await;
        Ok(())
    }
}

impl UdpListener {
    /// bind to any
    pub async fn bind() -> Result<Self> {
        let (ip, port) = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), UPD_MESSAGE_PORT);
        let addr = SocketAddr::new(ip, port);
        LOGGER.info(format!("Binding UDP listener to {}", addr));
        let socket = UdpSocket::bind(addr).await?;
        socket.set_broadcast(true)?;
        Ok(Self { socket })
    }

    /// Get the local address this listener is bound to.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Start an infinite receive loop in a background task.
    /// The provided handler will be invoked for each received datagram with the payload and peer addr.
    /// The task runs until a shutdown signal is received.
    pub fn into_task(
        self,
        mut on_packet: impl FnMut(Bytes, SocketAddr) + Send + 'static,
    ) -> ListenerHandle {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024]; // max UDP payload size safe buffer
            loop {
                tokio::select! {
                    biased;
                    _ = &mut shutdown_rx => {
                        // graceful break
                        LOGGER.info("Upd listener received shutdown signal, exiting...");
                        break;
                    }
                    res = self.socket.recv_from(&mut buf) => {
                        match res {
                            Ok((n, peer)) => {
                                let data = Bytes::copy_from_slice(&buf[..n]);
                                LOGGER.debug(format!("Received UDP packet from {:?} with length {}", peer, n));
                                on_packet(data, peer);
                            }
                            Err(_e) => {
                                // If receiving fails, continue the loop. In real app, consider logging and backoff.
                                // For minimal implementation, just continue to keep listening.
                                LOGGER.debug(format!("Failed to receive UDP packet {:?}", _e));
                                continue;
                            }
                        }
                    }
                }
            }
        });
        ListenerHandle {
            handle,
            shutdown_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::sender::{NetworkSenderCore, SenderConfig};
    use bytes::Bytes;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn udp_listener_receives_one_datagram() -> Result<()> {
        // Bind listener on a local ephemeral port
        let listener = UdpListener::bind().await?;
        let dest = listener.local_addr().unwrap();

        // Channel to capture received payload
        let (tx, rx) = oneshot::channel::<Bytes>();
        let mut tx_opt = Some(tx);
        let handle = listener.into_task(move |bytes, _peer| {
            if let Some(tx) = tx_opt.take() {
                let _ = tx.send(bytes);
            }
        });

        // Send one datagram to the listener
        let sender_server = NetworkSenderCore::new_queue_worker(SenderConfig {
            queue_bound: 16,
            connect_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
        });
        sender_server
            .sender()
            .send(dest, Bytes::from_static(b"hello-listener"))
            .await?;

        // Verify receipt
        let got = rx.await.expect("listener should forward payload");
        assert_eq!(&got[..], b"hello-listener");

        // Shutdown sender and the listener task gracefully
        let _ = sender_server.shutdown().await;
        let _ = handle.shutdown().await?;

        Ok(())
    }
}
