use crate::constants::TCP_FILE_PORT;
use crate::err::Result;
use crate::global_var::LOGGER;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::{TcpListener as TokioTcpListener, TcpStream as TokioTcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// TcpListener binds to an address and continuously accepts TCP connections.
/// It can bind from a constant port or from an explicit SocketAddr (useful for tests).
pub struct TcpListener {
    listener: TokioTcpListener,
}

/// Re-export Tokio's TcpStream so callers can interact with the stream directly.
pub type TcpStream = TokioTcpStream;

/// Handle to a running TCP listener task, allowing graceful shutdown.
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

impl TcpListener {
    /// Bind to 0.0.0.0:TCP_FILE_PORT
    pub async fn bind() -> Result<Self> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), TCP_FILE_PORT);
        LOGGER.info(format!("Binding TCP listener to {}", addr));
        let listener = TokioTcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    /// Bind to a specific socket address (useful for tests to use ephemeral ports).
    pub async fn bind_on(addr: SocketAddr) -> Result<Self> {
        LOGGER.info(format!("Binding TCP listener to {}", addr));
        let listener = TokioTcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    /// Get the local address this listener is bound to.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Start an infinite accept loop in a background task.
    /// The provided handler will be invoked for each accepted connection with the stream and peer addr.
    /// The task runs until a shutdown signal is received.
    pub fn into_task(
        self,
        mut on_conn: impl FnMut(TcpStream, SocketAddr) + Send + 'static,
    ) -> ListenerHandle {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut shutdown_rx => {
                        LOGGER.info("Tcp listener received shutdown signal, exiting...");
                        break;
                    }
                    res = self.listener.accept() => {
                        match res {
                            Ok((stream, peer)) => {
                                LOGGER.debug(format!("Accepted TCP connection from {:?}", peer));
                                on_conn(stream, peer);
                            }
                            Err(e) => {
                                LOGGER.debug(format!("Failed to accept TCP connection {:?}", e));
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream as ClientTcpStream;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn tcp_listener_accepts_one_connection_and_reads_payload() -> Result<()> {
        // Bind listener on a local ephemeral port (localhost:0)
        let listener =
            TcpListener::bind_on(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let dest = listener.local_addr().unwrap();

        // Channel to capture received payload
        let (tx, rx) = oneshot::channel::<Vec<u8>>();
        let mut tx_opt = Some(tx);
        let handle = listener.into_task(move |mut stream, _peer| {
            // Read the entire stream to EOF in a separate task and forward bytes
            if let Some(tx) = tx_opt.take() {
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let _ = stream.read_to_end(&mut buf).await;
                    let _ = tx.send(buf);
                });
            }
        });

        // Connect as a client and send a small payload
        let mut client = ClientTcpStream::connect(dest).await?;
        client.write_all(b"hello-tcp").await?;
        // Gracefully shutdown the write half to let server see EOF
        let _ = client.shutdown().await;

        // Verify receipt
        let got = rx.await.expect("listener should forward payload");
        assert_eq!(&got[..], b"hello-tcp");

        // Shutdown the listener task gracefully
        let _ = handle.shutdown().await?;
        Ok(())
    }
}
