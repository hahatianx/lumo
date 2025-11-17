use crate::core::protocol::file_send;
use crate::core::protocol::file_sync::FileSync;
use crate::core::tasks::{AsyncHandleable, JobStatus};
use crate::err::Result;
use crate::fs::claim_pending_pull;
use crate::global_var::LOGGER;
use crate::network::TcpConn;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub struct SendFileTask {
    tcp_conn: TcpConn,
}

impl SendFileTask {
    pub fn new(stream: TcpStream, peer: SocketAddr) -> Self {
        Self {
            tcp_conn: TcpConn::new(stream, peer),
        }
    }
}

#[async_trait]
impl AsyncHandleable for SendFileTask {
    async fn handle(&mut self) -> Result<()> {
        // 1. Read 1024 bytes and decrypt/deserialize into FileSync
        let sync_bytes = self.tcp_conn.read_bytes(1024).await?;
        let sync =
            FileSync::from_encryption(sync_bytes.to_vec().into_boxed_slice()).map_err(|e| {
                LOGGER.warn(format!(
                    "Failed to decrypt/deserialize FileSync from {}: {:?}",
                    self.tcp_conn.peer_addr(),
                    e
                ));
                // Connection considered bad; stop handling.
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to decrypt/deserialize FileSync",
                )
            })?;

        // 2. Validate FileSync by calling is_valid
        if !sync.is_valid() {
            LOGGER.warn(format!(
                "Received invalid/expired FileSync from {} for nonce {:x}",
                self.tcp_conn.peer_addr(),
                sync.nonce()
            ));
            return Err(format!(
                "The received FileSync is invalid or expired. Request: {:?}",
                sync
            )
            .into());
        }

        // 3 & 4. Check nonce validity and claim the job from file_pull map
        let nonce = sync.nonce();
        let mut pending_pull = match claim_pending_pull(nonce).await {
            Some(pending) => pending,
            None => {
                LOGGER.warn(format!(
                    "Received FileSync for nonce {:x} from {} that is not pending",
                    nonce,
                    self.tcp_conn.peer_addr()
                ));
                return Err(
                    format!("The received FileSync is not pending. Request: {:?}", sync).into(),
                );
            }
        };

        let handle = pending_pull.handle.take().unwrap();
        let mut callback = handle.take_over().await.map_err(|e| {
            LOGGER.error(format!(
                "Failed to take over claimable job for nonce {:x}: {:?}",
                nonce, e
            ));
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to take over claimable job",
            )
        })?;

        // 5. Send the file using protocol helper
        let res = file_send::send_file(
            nonce,
            pending_pull.temp_file_path.clone(),
            &mut self.tcp_conn,
        )
        .await;

        // 6. End the claimed job with proper status and log errors if any
        match res {
            Ok(()) => {
                // Mark job as completed
                if let Err(e) = callback(JobStatus::Completed, String::new()).await {
                    LOGGER.error(format!(
                        "Failed to update job status to Completed for nonce {:x}: {:?}",
                        nonce, e
                    ));
                }
            }
            Err(err) => {
                LOGGER.warn(format!(
                    "File send failed for nonce {:x} to {}: {:?}",
                    nonce,
                    self.tcp_conn.peer_addr(),
                    err
                ));
                if let Err(e) = callback(
                    JobStatus::Failed,
                    format!(
                        "Send file {:?} failed: {:?}",
                        &pending_pull.original_path, err
                    ),
                )
                .await
                {
                    LOGGER.error(format!(
                        "Failed to update job status to Failed for nonce {:x}: {:?}",
                        nonce, e
                    ));
                }
            }
        }

        Ok(())
    }
}
