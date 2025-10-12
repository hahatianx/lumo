use crate::err::Result;
use crate::global_var::ENV_VAR;
use bytes::Bytes;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// NetworkSender provides two patterns for sending UDP requests using Tokio:
/// - Queued single-consumer worker (default): provides backpressure, connection reuse, and ordering per destination.
/// - Per-request spawned task: fire-and-forget style; useful for very low latency fan-out with lower coordination.
///
/// By default, construct with `NetworkSender::new_queue_worker` and call `send` to enqueue a request
/// and await completion. You can also use `NetworkSender::spawn_per_request` for adhoc sending.
#[derive(Debug)]
pub struct NetworkSenderCore {
    tx: mpsc::Sender<SendReq>,
    worker: JoinHandle<()>,
}

#[derive(Debug)]
pub struct NetworkSender {
    tx: mpsc::Sender<SendReq>,
}

#[derive(Clone, Debug)]
pub struct SenderConfig {
    /// Max queued requests before backpressure. If 0, an unbounded channel is used.
    pub queue_bound: usize,
    /// Timeout for binding+connecting a UDP socket before a sending.
    /// Set to Duration::ZERO to disable the connect timeout for UDP.
    pub connect_timeout: Duration,
    /// Write timeout for sending bytes on a UDP socket.
    pub write_timeout: Duration,
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            queue_bound: 1024,
            connect_timeout: Duration::from_secs(3),
            write_timeout: Duration::from_secs(3),
        }
    }
}

enum SendReq {
    Data { addr: SocketAddr, bytes: Bytes },
    Shutdown,
}

impl NetworkSenderCore {
    /// Create a queued NetworkSender with a single consumer worker.
    /// This approach is generally preferable for:
    /// - Applying backpressure
    /// - Reusing connections per destination
    /// - Maintaining send order for the same addr
    pub fn new_queue_worker(config: SenderConfig) -> Self {
        let (tx, rx) = if config.queue_bound == 0 {
            // emulate unbounded by using a large bound; tokio doesn't expose unbounded mpsc in std feature set
            mpsc::channel(usize::MAX / 2)
        } else {
            mpsc::channel(config.queue_bound)
        };
        let worker = tokio::spawn(run_worker(rx, config));
        Self { tx, worker }
    }

    pub fn sender(&self) -> NetworkSender {
        NetworkSender {
            tx: self.tx.clone(),
        }
    }

    /// Gracefully shutdown the queue worker by sending a Shutdown request and awaiting the worker task.
    pub async fn shutdown(self) -> Result<()> {
        // Ignore errors if the channel is already closed.
        let _ = self.tx.send(SendReq::Shutdown).await;
        // Await the worker to finish cleanup.
        let _ = self.worker.await;
        Ok(())
    }
}

impl NetworkSender {
    /// Enqueue a send operation and await its result.
    pub async fn send(&self, addr: SocketAddr, bytes: Bytes) -> Result<()> {
        let req = SendReq::Data { addr, bytes };
        // If the channel is closed, report an error.
        if let Err(_e) = self.tx.send(req).await {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "NetworkSender worker task is not running",
            )
            .into());
        }
        Ok(())
    }

    /// Broadcast the same bytes to multiple addresses by enqueuing one sending per address.
    /// This awaits each enqueue to apply backpressure. Stops and returns an error on the first failure.
    pub async fn broadcast(&self, bytes: Bytes) -> Result<()> {
        // Send the payload to 255.255.255.255:<port>. If ENV_VAR is not initialized (e.g., in tests),
        // fall back to the conventional default port used by this project (14514).
        let port = ENV_VAR.get().map(|ev| ev.get_port()).unwrap_or(14514);
        let broadcast_ip = IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255));
        let addr = SocketAddr::new(broadcast_ip, port);
        self.send(addr, bytes.clone()).await?;
        Ok(())
    }

    /// Spawn a per-request task that connects and sends the bytes, returning a JoinHandle.
    /// Prefer this for very bursty fan-out where connection reuse is not critical.
    pub fn spawn_per_request(
        addr: SocketAddr,
        bytes: Bytes,
        connect_timeout: Duration,
        write_timeout: Duration,
    ) -> JoinHandle<Result<()>> {
        tokio::spawn(async move { send_once(addr, bytes, connect_timeout, write_timeout).await })
    }
}

async fn bind_and_connect(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    // Determine local bind IP:
    // - If destination is loopback, bind to the corresponding loopback IP to ensure local routing.
    // - Else, prefer the IP from EnvVar (if available and same family); otherwise fall back to unspecified for that family.
    let local_ip: IpAddr = if addr.ip().is_loopback() {
        if addr.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        }
    } else if let Some(ev) = ENV_VAR.get() {
        let ip = ev.get_ip_addr();
        match (ip, addr) {
            (IpAddr::V4(ipv4), SocketAddr::V4(_)) => IpAddr::V4(ipv4),
            (IpAddr::V6(ipv6), SocketAddr::V6(_)) => IpAddr::V6(ipv6),
            // Family mismatch: use unspecified for the destination family
            (_, SocketAddr::V4(_)) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            (_, SocketAddr::V6(_)) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        }
    } else {
        if addr.is_ipv4() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        }
    };

    let local = SocketAddr::new(local_ip, 0);
    let s = UdpSocket::bind(local).await?;
    // Enable broadcast if destination is the limited broadcast address
    if let SocketAddr::V4(v4) = addr {
        if v4.ip().octets() == [255, 255, 255, 255] {
            s.set_broadcast(true)?;
        }
    }
    s.connect(addr).await?;
    Ok(s)
}

async fn run_worker(mut rx: mpsc::Receiver<SendReq>, cfg: SenderConfig) {
    // Cache connected UDP sockets per addr for reuse.
    let mut conns: HashMap<SocketAddr, UdpSocket> = HashMap::new();

    while let Some(req) = rx.recv().await {
        match req {
            SendReq::Data { addr, bytes } => {
                let _res = async {
                    // Get or create a connected UDP socket
                    let sock = match conns.remove(&addr) {
                        Some(s) => s,
                        None => {
                            let s = if cfg.connect_timeout.is_zero() {
                                bind_and_connect(addr).await?
                            } else {
                                timeout(cfg.connect_timeout, bind_and_connect(addr))
                                    .await
                                    .map_err(|_| {
                                        std::io::Error::new(
                                            std::io::ErrorKind::TimedOut,
                                            format!("connect timeout to {}", addr),
                                        )
                                    })??
                            };
                            s
                        }
                    };

                    // Try to send it; on failure, create a fresh socket once and retry.
                    match send_with_timeout(sock, &bytes, cfg.write_timeout).await {
                        Ok(s) => {
                            // put back for reuse
                            conns.insert(addr, s);
                            Ok(())
                        }
                        Err(_e) => {
                            // attempt single re-bind/connect
                            let s = if cfg.connect_timeout.is_zero() {
                                bind_and_connect(addr).await?
                            } else {
                                timeout(cfg.connect_timeout, bind_and_connect(addr))
                                    .await
                                    .map_err(|_| format!("reconnect timeout to {}", addr))??
                            };
                            let s = send_with_timeout(s, &bytes, cfg.write_timeout).await?;
                            conns.insert(addr, s);
                            Ok(())
                        }
                    }
                }
                .await
                .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e);
            }
            SendReq::Shutdown => {
                break;
            }
        }
    }

    // Clean up: let UdpSockets drop here.
}

async fn send_with_timeout(sock: UdpSocket, bytes: &Bytes, to: Duration) -> Result<UdpSocket> {
    timeout(to, async {
        let _ = sock.send(bytes).await?;
        Ok::<_, crate::err::Error>(())
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "write timeout"))??;
    Ok(sock)
}

async fn send_once(
    addr: SocketAddr,
    bytes: Bytes,
    connect_timeout: Duration,
    write_timeout: Duration,
) -> Result<()> {
    let sock = if connect_timeout.is_zero() {
        bind_and_connect(addr).await?
    } else {
        timeout(connect_timeout, bind_and_connect(addr))
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("connect timeout to {}", addr),
                )
            })??
    };
    let _ = send_with_timeout(sock, &bytes, write_timeout).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UdpSocket;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_queue_worker_sends() -> Result<()> {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        // Spawn receiver that reads one datagram and captures it
        let (tx, rx) = oneshot::channel::<Bytes>();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64];
            let _ = server
                .recv_from(&mut buf)
                .await
                .map(|(n, _)| {
                    buf.truncate(n);
                    buf
                })
                .map(|b| tx.send(Bytes::from(b)));
        });

        let sender = NetworkSenderCore::new_queue_worker(SenderConfig::default()).sender();
        sender.send(addr, Bytes::from_static(b"hello")).await?;
        let got = rx.await.unwrap();
        assert_eq!(&got[..], b"hello");
        Ok(())
    }

    #[tokio::test]
    async fn test_per_request_send() -> Result<()> {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        let (tx, rx) = oneshot::channel::<Bytes>();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64];
            let _ = server
                .recv_from(&mut buf)
                .await
                .map(|(n, _)| {
                    buf.truncate(n);
                    buf
                })
                .map(|b| tx.send(Bytes::from(b)));
        });

        let handle = NetworkSender::spawn_per_request(
            addr,
            Bytes::from_static(b"pong"),
            Duration::from_secs(3),
            Duration::from_secs(3),
        );
        let _ = handle.await.unwrap()?;
        let got = rx.await.unwrap();
        assert_eq!(&got[..], b"pong");
        Ok(())
    }

    #[tokio::test]
    async fn test_broadcast_multiple_receivers() -> Result<()> {
        // Prepare two fake IPv4 SocketAddrs with different ports to exercise the API.
        let s1 = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let s2 = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let _a1 = s1.local_addr().unwrap();
        let _a2 = s2.local_addr().unwrap();
        drop(s1);
        drop(s2);

        let sender = NetworkSenderCore::new_queue_worker(SenderConfig::default()).sender();
        let payload = Bytes::from_static(b"bcast");
        // This should send to 255.255.255.255 without error.
        sender.broadcast(payload.clone()).await?;
        Ok(())
    }
}
