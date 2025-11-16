use crate::core::protocol::file_sync::{FileSyncAck, FileSyncError};
use crate::err::Result;
use crate::fs::LumoFile;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::TcpConn;
use crate::utilities::crypto::f_to_encryption;
use bytes::Bytes;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

type Nonce = u64;
type Checksum = u64;

struct FileSendTracker {
    nonce: Nonce,

    source_path: PathBuf,
    enc_tmp_path: PathBuf,
}

impl FileSendTracker {
    pub fn new(nonce: Nonce, source_path: PathBuf) -> Self {
        // Build base path: <working_dir>/.disc/tmp_downloads
        let base = PathBuf::from(ENV_VAR.get().unwrap().get_temp_downloads_dir());
        let enc_tmp_path = base.join(format!("recv-{}-{}.cipher", nonce, rand::random::<u64>()));

        FileSendTracker {
            nonce,
            source_path,
            enc_tmp_path,
        }
    }

    async fn send_file(&self, conn: &mut TcpConn, total_size: u64) -> Result<()> {
        let file = tokio::fs::File::open(&self.enc_tmp_path).await?;

        LOGGER.trace(format!(
            "Sending file {} to peer",
            self.enc_tmp_path.display()
        ));

        let write_timeout = conn.get_write_timeout();

        let stream = &mut conn.stream;
        tokio::time::timeout(
            write_timeout,
            tokio::io::copy(&mut file.take(total_size), stream),
        )
        .await??;

        Ok(())
    }

    pub async fn send(&self, conn: &mut TcpConn) -> std::result::Result<(), FileSyncError> {
        f_to_encryption(&self.source_path, &self.enc_tmp_path, || {
            let iv: [u8; 16] = rand::random();
            Ok(iv)
        })
        .map_err(|e| {
            LOGGER.warn(format!("Encryption source file failed: {:?}", e));
            FileSyncError::SystemError
        })?;

        let lumo_file = LumoFile::new(self.enc_tmp_path.clone())
            .await
            .map_err(|e| {
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

        self.send_file(conn, total_size).await.map_err(|e| {
            LOGGER.warn(format!("File send error {:?}", e));
            FileSyncError::AbortedByPeer
        })?;

        Ok(())
    }
}

impl Drop for FileSendTracker {
    fn drop(&mut self) {
        LOGGER.info(format!(
            "FileSendTracker dropped: nonce={}, paths [{:?}] removed",
            self.nonce, &self.enc_tmp_path
        ));
        std::fs::remove_file(&self.enc_tmp_path).ok();
    }
}

/// Public helper to send an encrypted file over an existing TcpConn.
/// The source file will be encrypted into a temporary cipher file and then streamed.
/// The temporary cipher file is removed when the tracker is dropped.
pub async fn send_file(
    nonce: u64,
    source_path: PathBuf,
    conn: &mut TcpConn,
) -> std::result::Result<(), FileSyncError> {
    let tracker = FileSendTracker::new(nonce, source_path);
    tracker.send(conn).await
}
