use crate::core::protocol::file_sync::{FileSyncAck, FileSyncError};
use crate::err::Result;
use crate::fs::{LumoFile, fs_lock};
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::TcpConn;
use crate::utilities::crypto::f_to_encryption;
use bytes::Bytes;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::time::error::Elapsed;

type Nonce = u64;
type Checksum = u64;

pub struct FileSendSummary {
    pub nonce: Nonce,
    pub file_size: u64,
    pub checksum: Checksum,
    pub elapsed: std::time::Duration,
}

impl FileSendSummary {
    fn new(nonce: Nonce, file_size: u64, checksum: Checksum, elapsed: std::time::Duration) -> Self {
        Self {
            nonce,
            file_size,
            checksum,
            elapsed,
        }
    }
}

struct FileSendTracker {
    nonce: Nonce,

    source_path: PathBuf,
}

impl FileSendTracker {
    pub fn new(nonce: Nonce, source_path: PathBuf) -> Self {
        // Build base path: <working_dir>/.disc/tmp_downloads
        let base = PathBuf::from(ENV_VAR.get().unwrap().get_temp_downloads_dir());

        FileSendTracker { nonce, source_path }
    }

    async fn send_file(
        &self,
        conn: &mut TcpConn,
        total_size: u64,
    ) -> std::result::Result<u64, FileSyncError> {
        // Acquire a read lock on the encrypted temp file to ensure consistency across processes
        let guard = fs_lock::RwLock::new(&self.source_path)
            .read()
            .await
            .map_err(|e| {
                LOGGER.warn(format!("Failed to lock file: {:?}", e));
                FileSyncError::SystemError
            })?;

        LOGGER.trace(format!(
            "Sending file {} to peer",
            self.source_path.display()
        ));

        // Clone the underlying std::fs::File so we can create an async file while keeping the lock guard alive
        let std_file = guard.try_clone().map_err(|e| {
            LOGGER.warn(format!("Failed to clone file: {:?}", e));
            FileSyncError::SystemError
        })?;
        let file = tokio::fs::File::from_std(std_file);

        // Assume a 5 MB/s transfer rate as the low bound
        let write_timeout = conn.get_write_timeout().max(std::time::Duration::from_secs(
            total_size / (1024 * 1024 * 5) as u64,
        ));
        let stream = &mut conn.stream;

        // Limit the reader to the expected total size and copy to the peer with a timeout
        LOGGER.debug(format!(
            "Starting file transfer, total_size: {} bytes, expected time {}",
            total_size,
            write_timeout.as_secs_f64() * 1000.0
        ));
        let sz = tokio::time::timeout(
            write_timeout,
            tokio::io::copy(&mut file.take(total_size), stream),
        )
        .await
        .map_err(|e| {
            LOGGER.warn(format!("File transfer timed out {:?}", e));
            FileSyncError::Timeout
        })?
        .map_err(|e| {
            LOGGER.warn(format!("Failed writing to peer: {:?}", e));
            FileSyncError::AbortedByPeer
        })?;
        Ok(sz)
    }

    pub async fn send(
        &self,
        conn: &mut TcpConn,
    ) -> std::result::Result<FileSendSummary, FileSyncError> {
        let lumo_file = LumoFile::new(self.source_path.clone()).await.map_err(|e| {
            LOGGER.warn(format!("LumoFile create error: {:?}", e));
            FileSyncError::SystemError
        })?;

        let total_size = lumo_file.size;
        let checksum = lumo_file.get_checksum().await.map_err(|e| {
            LOGGER.warn(format!("LumoFile checksum error: {:?}", e));
            FileSyncError::SystemError
        })?;

        let sync_ack = FileSyncAck::new(self.nonce, Some(checksum), total_size);
        conn.send_bytes(Bytes::from(sync_ack.to_encryption().map_err(|e| {
            LOGGER.warn(format!("Encrypting ack package failed: {:?}", e));
            FileSyncError::SystemError
        })?))
        .await
        .map_err(|e| {
            LOGGER.warn(format!("Sending sync ack failed {:?}", e));
            FileSyncError::AbortedByPeer
        })?;

        // pause here to allow the peer to read the ACK from the server before we start sending the file
        let _ = conn.read_bytes(10).await.map_err(|e| {
            LOGGER.warn(format!(
                "Waiting for peer to confirm sync ack aborted {:?}",
                e
            ));
            FileSyncError::AbortedByPeer
        })?;

        let start_time = std::time::Instant::now();
        self.send_file(conn, total_size).await.map_err(|e| {
            LOGGER.warn(format!("File send error {:?}", e));
            FileSyncError::AbortedByPeer
        })?;

        let summary = FileSendSummary::new(self.nonce, total_size, checksum, start_time.elapsed());
        Ok(summary)
    }
}

/// Public helper to send an encrypted file over an existing TcpConn.
/// The source file will be encrypted into a temporary cipher file and then streamed.
/// The temporary cipher file is removed when the tracker is dropped.
pub async fn send_file(
    nonce: u64,
    source_path: PathBuf,
    conn: &mut TcpConn,
) -> std::result::Result<FileSendSummary, FileSyncError> {
    let tracker = FileSendTracker::new(nonce, source_path);
    tracker.send(conn).await
}
