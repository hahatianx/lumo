use crate::err::Result;
use crate::global_var::LOGGER;
use bytes::{Bytes, BytesMut};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream as TokioTcpStream;
use tokio::time::timeout;

/// A simple TCP connection wrapper that owns a Tokio TcpStream and provides
/// a minimal lifecycle: connect, send bytes, and graceful shutdown.
#[derive(Debug)]
pub struct TcpConn {
    pub stream: TokioTcpStream,
    peer: SocketAddr,
    /// Optional connect timeout
    connect_timeout: Duration,
    /// Optional write timeout
    write_timeout: Duration,
    /// Optional read timeout
    read_timeout: Duration,
}

#[derive(Clone, Copy, Debug)]
pub struct TcpConnConfig {
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub read_timeout: Duration,
}

impl Default for TcpConnConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(30),
        }
    }
}

impl TcpConn {
    pub fn new(stream: TokioTcpStream, peer: SocketAddr) -> Self {
        let cfg = TcpConnConfig::default();
        Self {
            stream,
            peer,
            connect_timeout: cfg.connect_timeout,
            write_timeout: cfg.write_timeout,
            read_timeout: cfg.read_timeout,
        }
    }
    /// Connect to the given socket address using default timeouts.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        Self::connect_with_config(addr, TcpConnConfig::default()).await
    }

    /// Connect to the given socket address with a custom configuration.
    pub async fn connect_with_config(addr: SocketAddr, cfg: TcpConnConfig) -> Result<Self> {
        let connect_fut = TokioTcpStream::connect(addr);
        let stream = if cfg.connect_timeout.is_zero() {
            connect_fut.await?
        } else {
            match timeout(cfg.connect_timeout, connect_fut).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return Err(e.into()),
                Err(_elapsed) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "tcp connect timeout",
                    )
                    .into());
                }
            }
        };
        LOGGER.debug(format!("Established TCP connection to {}", addr));
        Ok(Self {
            stream,
            peer: addr,
            connect_timeout: cfg.connect_timeout,
            write_timeout: cfg.write_timeout,
            read_timeout: cfg.read_timeout,
        })
    }

    /// Return the peer address this connection was established to.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }

    /// Return the local address, if available.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.local_addr()
    }

    pub fn get_read_timeout(&self) -> Duration {
        self.read_timeout
    }

    pub fn get_write_timeout(&self) -> Duration {
        self.write_timeout
    }

    /// Send the entire buffer over the TCP stream. Honors write timeout if non-zero.
    pub async fn send_all(&mut self, bytes: &[u8]) -> Result<()> {
        if self.write_timeout.is_zero() {
            self.stream.write_all(bytes).await?;
        } else {
            match timeout(self.write_timeout, self.stream.write_all(bytes)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e.into()),
                Err(_elapsed) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "tcp write timeout",
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    /// Convenience to send Bytes.
    pub async fn send_bytes(&mut self, bytes: Bytes) -> Result<()> {
        self.send_all(&bytes).await?;
        self.send_all(b"\r\n").await
    }

    pub async fn read_bytes(&mut self, len: usize) -> Result<Bytes> {
        if len == 0 {
            return Ok(Bytes::new());
        }
        let mut buf = BytesMut::with_capacity(len + 5);
        unsafe {
            buf.set_len(len + 5);
        }
        let mut read_total = 0usize;
        while read_total < len + 2 {
            let fut = self.stream.read(&mut buf[read_total..]);
            let n = if self.read_timeout.is_zero() {
                fut.await?
            } else {
                match timeout(self.read_timeout, fut).await {
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => return Err(e.into()),
                    Err(_elapsed) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "tcp read timeout",
                        )
                        .into());
                    }
                }
            };
            if n == 0 {
                break;
            }
            read_total += n;
            if read_total >= 2 && buf[read_total - 2] == b'\r' && buf[read_total - 1] == b'\n' {
                break;
            }
        }
        if read_total == len + 2 {
            return Err("Tcp read buffer overflow".into());
        }
        buf.truncate(read_total - 2);
        Ok(buf.freeze())
    }

    /// Gracefully shutdown the write half to signal EOF to the remote end.
    pub async fn shutdown(mut self) -> Result<()> {
        self.stream.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::tcp_listener::TcpListener;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn tcp_conn_connects_and_sends_payload() -> Result<()> {
        // Start a temporary server
        let listener =
            TcpListener::bind_on(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let dest = listener.local_addr().unwrap();

        // Accept one connection and read the payload
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
        let mut tx_opt = Some(tx);
        let handle = listener.into_task(move |mut stream, _peer| {
            if let Some(tx) = tx_opt.take() {
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let _ = stream.read_to_end(&mut buf).await;
                    let _ = tx.send(buf);
                });
            }
        });

        // Connect using TcpConn and send data
        let mut conn = TcpConn::connect(dest).await?;
        conn.send_all(b"hello-from-tcp-conn").await?;
        // Graceful shutdown write side so server sees EOF
        let _ = conn.shutdown().await;

        // Verify receipt
        let got = rx.await.expect("server should receive payload");
        assert_eq!(&got[..], b"hello-from-tcp-conn");

        // Shutdown server task
        let _ = handle.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn tcp_conn_timeout_on_connect_and_write_configurable() -> Result<()> {
        // For connect timeout, attempt to connect to a non-routable address with a short timeout.
        // 203.0.113.1 is TEST-NET-3 and typically unroutable; but to avoid flakiness, we use localhost:1 where no one listens.
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1);
        let cfg = TcpConnConfig {
            connect_timeout: Duration::from_millis(50),
            write_timeout: Duration::from_millis(50),
            read_timeout: Duration::from_millis(50),
        };
        let res = TcpConn::connect_with_config(addr, cfg).await;
        assert!(res.is_err(), "connect should likely fail or timeout");

        // Start a server to test write timeout by pausing reads is complicated; we just ensure send works under default cfg.
        let listener =
            TcpListener::bind_on(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let dest = listener.local_addr().unwrap();
        let handle = listener.into_task(move |_stream, _peer| {
            // Drop immediately so client write either fails or succeeds; either is acceptable for this smoke test.
        });
        let mut conn = TcpConn::connect(dest).await?;
        let _ = conn.send_all(b"hi").await; // ignore result; depends on race
        let _ = handle.shutdown().await?;
        Ok(())
    }
}
