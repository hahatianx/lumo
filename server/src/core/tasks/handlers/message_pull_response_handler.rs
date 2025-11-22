use crate::constants::TCP_FILE_PORT;
use crate::core::protocol::file_recv::{FileRecvSummary, FileRecvTracker};
use crate::core::protocol::file_send::FileSendSummary;
use crate::core::protocol::file_sync::FileSyncError;
use crate::core::tasks::handlers::IGNORE_SELF;
use crate::core::tasks::{AsyncHandleable, JobStatus, NetworkHandleable};
use crate::fs::file::get_file_checksum;
use crate::fs::{PendingFileDownloadTask, claim_pending_download};
use crate::global_var::LOGGER;
use crate::network::TcpConn;
use crate::network::protocol::messages::pull_response_message::{
    PullDecision, PullResponseMessage,
};
use crate::utilities::format::size_to_human_readable;
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use std::net::{IpAddr, SocketAddr};

type Challenge = u64;
type Nonce = u64;
type Checksum = u64;

enum DownloadFileError {
    RejectedByPeer(String),
    NetworkError(String),
    FileMalformed,
    FileFromChecksumMismatch,
    FileChecksumMismatch,
    SystemError(String),
}

impl Debug for DownloadFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadFileError::RejectedByPeer(reason) => write!(f, "RejectedByPeer: {}", reason),
            DownloadFileError::NetworkError(reason) => write!(f, "NetworkError: {}", reason),
            DownloadFileError::FileMalformed => write!(f, "FileMalformed"),
            DownloadFileError::FileFromChecksumMismatch => write!(f, "FileFromChecksumMismatch"),
            DownloadFileError::FileChecksumMismatch => write!(f, "FileChecksumMismatch"),
            DownloadFileError::SystemError(reason) => write!(f, "SystemError: {}", reason),
        }
    }
}

impl PullResponseMessage {
    async fn download_and_replace(
        &self,
        pending_file_download: &PendingFileDownloadTask,
        decision: PullDecision,
        conn: TcpConn,
    ) -> std::result::Result<FileRecvSummary, DownloadFileError> {
        let nonce = match decision {
            PullDecision::Accept(c, n) => n,
            PullDecision::Reject(c, r) => {
                return Err(DownloadFileError::RejectedByPeer(format!(
                    "Rejected by peer with reason: {}",
                    r
                )));
            }
        };

        let from_checksum = pending_file_download.from_checksum;
        let to_checksum = pending_file_download.to_checksum;

        let file_download_tracker = FileRecvTracker::new(nonce, to_checksum.into());
        let summary = file_download_tracker
            .recv(conn)
            .await
            .map_err(|e| match e {
                FileSyncError::AbortedByPeer => {
                    DownloadFileError::NetworkError("File download aborted by peer".to_string())
                }
                FileSyncError::Timeout => {
                    DownloadFileError::NetworkError("File download timed out".to_string())
                }
                FileSyncError::FileMalformed => DownloadFileError::FileMalformed,
                FileSyncError::SystemError => DownloadFileError::SystemError(
                    "File download failed due to system error".to_string(),
                ),
            })?;

        if from_checksum.has_expected() {
            // from checksum is not None, meaning we are replacing the file whose checksum is from_checksum

            let write_guard = crate::fs::RwLock::new(pending_file_download.file_path.clone())
                .write()
                .await
                .map_err(|e| {
                    DownloadFileError::SystemError(format!("Failed to lock file: {:?}", e))
                })?;

            LOGGER.debug(format!(
                "File {} locked successfully",
                pending_file_download.file_path.display()
            ));

            let (_, _, checksum, _write_guard) =
                get_file_checksum(write_guard).await.map_err(|e| {
                    DownloadFileError::SystemError(format!("Failed to get file checksum: {:?}", e))
                })?;

            LOGGER.debug(format!(
                "File {} checksum fetched.",
                pending_file_download.file_path.display()
            ));

            if from_checksum.not_match_expected(&checksum) {
                return Err(DownloadFileError::FileFromChecksumMismatch);
            }

            LOGGER.debug(format!(
                "Moving {} to {}",
                &summary.file_path.display(),
                pending_file_download.file_path.display()
            ));

            tokio::fs::rename(&summary.file_path, &pending_file_download.file_path)
                .await
                .map_err(|e| {
                    DownloadFileError::SystemError(format!("Failed to rename file: {:?}", e))
                })?;

            LOGGER.debug(format!(
                "File {} copied from temp successfully",
                pending_file_download.file_path.display()
            ));
        } else {
            crate::utilities::disk_op::fs_rename(
                &summary.file_path,
                &pending_file_download.file_path,
            )
            .map_err(|e| {
                DownloadFileError::SystemError(format!("Failed to rename file: {:?}", e))
            })?;
        }

        Ok(summary)
    }

    async fn process_file_download(&self, decision: PullDecision) -> crate::err::Result<()> {
        let challenge = match decision {
            PullDecision::Accept(c, _) => c,
            PullDecision::Reject(c, _) => c,
        };

        // 1. Claim the pending download task by challenge
        let mut pending = match claim_pending_download(challenge).await {
            Some(p) => p,
            None => {
                let msg = format!("No pending download found for challenge {}", challenge);
                LOGGER.warn(&msg);
                return Err(msg.into());
            }
        };

        // Take over the claimable job to report status
        let handle = pending.handle.take().ok_or_else(|| {
            LOGGER.error(format!("No handle found for challenge {}", challenge));
            std::io::Error::new(std::io::ErrorKind::Other, "No handle found for challenge")
        })?;
        let mut callback = handle.take_over().await.map_err(|e| {
            LOGGER.error(format!(
                "Failed to take over download job for challenge {}: {:?}",
                challenge, e
            ));
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to take over claimable job",
            )
        })?;

        // 2. Setup TCP connection to sender
        let ip: IpAddr = self.from_ip.parse().map_err(|e| {
            LOGGER.warn(format!(
                "Invalid from_ip '{}' in PullResponseMessage: {:?}",
                self.from_ip, e
            ));
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid from_ip")
        })?;
        let addr = SocketAddr::new(ip, TCP_FILE_PORT);
        let conn = TcpConn::connect(addr).await.map_err(|e| {
            LOGGER.warn(format!("Failed to connect to {}: {:?}", addr, e));
            std::io::Error::new(std::io::ErrorKind::Other, "tcp connect failed")
        })?;

        // 3. Download and replace the file
        match self.download_and_replace(&pending, decision, conn).await {
            Ok(summary) => {
                LOGGER.info(format!(
                    "File downloaded successfully for challenge {},",
                    challenge
                ));
                let download_speed = size_to_human_readable(
                    (summary.file_size as f64 / summary.download_time.as_secs_f64()) as u64,
                );
                let decrypt_speed = size_to_human_readable(
                    (summary.file_size as f64 / summary.decrypt_time.as_secs_f64()) as u64,
                );
                let msg = format!(
                    "File downloaded successfully, file size: {}, download speed: {}, decrypt speed: {}",
                    summary.file_size, download_speed, decrypt_speed
                );
                callback(
                    JobStatus::Completed,
                    msg,
                )
                .await?;
            }
            Err(e) => {
                LOGGER.warn(format!(
                    "Failed to download file for challenge {}: {:?}",
                    challenge, e
                ));
                callback(JobStatus::Failed, format!("File download failed: {:?}", e)).await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl AsyncHandleable for PullResponseMessage {
    async fn handle(&mut self) -> crate::err::Result<()> {
        let resp = self.get_response()?;

        if resp.get_from_ip() != self.from_ip {
            LOGGER.warn(format!("Invalid from_ip in PullResponseMessage, the from_ip does not match the one in PullResponse: {} from message vs {} from encrypted response", resp.get_from_ip(), self.from_ip.to_string()));
            return Err("Invalid from_ip in PullResponseMessage".into());
        }

        if !resp.timestamp_valid() {
            LOGGER.warn(format!(
                "PullResponseMessage timestamp is too old, timestamp: {:?}",
                resp.get_timestamp()
            ));
            return Err("PullResponseMessage timestamp is too old".into());
        }

        let _ = self.process_file_download(*resp.get_decision()).await?;

        Ok(())
    }
}

impl NetworkHandleable for PullResponseMessage {
    fn should_ignore_by_sockaddr_peer(&self, peer: &SocketAddr) -> bool {
        IGNORE_SELF(peer)
    }
}
