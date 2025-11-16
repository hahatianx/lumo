use crate::core::protocol::file_sync::{FileSync, FileSyncAck, FileSyncError};
use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::TcpConn;
use crate::types::Expected;
use crate::utilities::crypto::f_from_encryption;
use bytes::Bytes;
use rand::random;
use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

type Nonce = u64;
type Checksum = u64;
pub struct FileRecvTracker {
    nonce: Nonce,
    expected_checksum: Expected<Checksum>,

    enc_tmp_path: PathBuf,
    target_path: PathBuf,
}

impl FileRecvTracker {
    pub fn new(nonce: Nonce, maybe_checksum: Option<Checksum>) -> Self {
        // Build base path: <working_dir>/.disc/tmp_downloads
        let base = PathBuf::from(ENV_VAR.get().unwrap().get_temp_downloads_dir());

        // Create two random, nonce-derived paths to avoid collisions
        let enc_tmp_path = base.join(format!("recv-{}-{}.cipher", nonce, random::<u64>()));
        let target_path = base.join(format!("recv-{}-{}.tmp", nonce, random::<u64>()));

        Self {
            nonce,
            expected_checksum: maybe_checksum.into(),
            enc_tmp_path,
            target_path,
        }
    }

    async fn sync(&self, conn: &mut TcpConn) -> Result<FileSyncAck> {
        let sync = FileSync::new(self.nonce).to_encryption()?;
        LOGGER.debug(format!("FileSync: {:?}", &sync));
        conn.send_bytes(Bytes::from(sync)).await?;
        let ack =
            FileSyncAck::from_encryption(conn.read_bytes(1024).await?.to_vec().into_boxed_slice())?;
        LOGGER.debug(format!("FileSyncAck: {:?}", &ack));
        Ok(ack)
    }

    async fn download_to_file(
        &self,
        conn: TcpConn,
        file: &mut File,
        total_size: u64,
    ) -> Result<u64> {
        LOGGER.info(format!(
            "Starting file receive: nonce={}, size={} bytes -> {}",
            self.nonce,
            total_size,
            self.target_path.display()
        ));

        let read_timeout = conn.get_read_timeout();
        let stream = conn.stream;
        let sz = tokio::time::timeout(
            read_timeout,
            tokio::io::copy(&mut stream.take(total_size), file),
        )
        .await
        .map_err(|e| {
            LOGGER.error(format!("Reading from stream timed out {:?}", e));
            std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e))
        })?
        .map_err(|e| {
            LOGGER.error(format!("Failed reading from connection: {:?}", e));
            std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e))
        })?;

        Ok(sz)
    }
    pub async fn recv(&self, mut conn: TcpConn) -> std::result::Result<&PathBuf, FileSyncError> {
        let ack = self.sync(&mut conn).await.map_err(|e| {
            LOGGER.error(format!("FileSync failed: {:?}", e));
            FileSyncError::AbortedByPeer
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.target_path)
            .await
            .map_err(|e| {
                LOGGER.error(format!(
                    "Failed to open target file {}: {:?}",
                    self.target_path.display(),
                    e
                ));
                FileSyncError::AbortedByPeer
            })?;

        self.download_to_file(conn, &mut file, ack.file_size())
            .await
            .map_err(|e| {
                LOGGER.error(format!("Failed to download file: {:?}", e));
                FileSyncError::AbortedByPeer
            })?;

        if let Err(e) = file.flush().await {
            LOGGER.error(format!("Failed to flush file: {:?}", e));
            return Err(FileSyncError::SystemError);
        }

        f_from_encryption(&self.enc_tmp_path, &self.target_path).map_err(|e| {
            LOGGER.error(format!("Failed to decrypt file: {:?}", e));
            FileSyncError::FileMalformed
        })?;

        Ok(&self.target_path)
    }
}

impl Drop for FileRecvTracker {
    fn drop(&mut self) {
        LOGGER.info(format!(
            "FileRecvTracker dropped: nonce={}, paths [{}, {}] are deleted.",
            self.nonce,
            self.target_path.display(),
            self.enc_tmp_path.display()
        ));
        std::fs::remove_file(&self.enc_tmp_path).ok();
        std::fs::remove_file(&self.target_path).ok();
    }
}
