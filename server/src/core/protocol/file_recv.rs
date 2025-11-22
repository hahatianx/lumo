use crate::core::protocol::file_sync::{FileSync, FileSyncAck, FileSyncError};
use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::TcpConn;
use crate::types::Expected;
use crate::utilities::crypto::f_from_encryption;
use crate::utilities::format::size_to_human_readable;
use bytes::Bytes;
use rand::random;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

type Nonce = u64;
type Checksum = u64;

pub struct FileRecvSummary {
    pub nonce: Nonce,
    pub file_size: u64,
    pub file_path: PathBuf,
    pub download_time: std::time::Duration,
    pub decrypt_time: std::time::Duration,
}

impl FileRecvSummary {
    fn new(
        nonce: Nonce,
        file_size: u64,
        file_path: PathBuf,
        download_time: std::time::Duration,
        decrypt_time: std::time::Duration,
    ) -> Self {
        Self {
            nonce,
            file_size,
            file_path,
            download_time,
            decrypt_time,
        }
    }
}

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
            FileSyncAck::from_encryption(conn.read_bytes(2048).await?.to_vec().into_boxed_slice())?;
        LOGGER.debug(format!("FileSyncAck: {:?}", &ack));
        conn.send_bytes(b"".to_vec().into()).await?;
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
            size_to_human_readable(total_size),
            self.target_path.display()
        ));

        // expected transfer lower bound: 5 MB / s
        // total_size / 1024 / 1024 / 5
        let read_timeout =
            Duration::from_secs(total_size / (1024 * 1024 * 5) + 1).max(conn.get_read_timeout());
        LOGGER.debug(format!("Read timeout: {:?}", read_timeout));
        let start_time = std::time::Instant::now();
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

        LOGGER.trace(format!(
            "File transfer completed, received {} bytes, time elapsed {:?}, transfer speed {}/s.",
            sz,
            start_time.elapsed(),
            size_to_human_readable((sz as f64 / start_time.elapsed().as_secs_f64()) as u64)
        ));

        Ok(sz)
    }
    pub async fn recv(
        &self,
        mut conn: TcpConn,
    ) -> std::result::Result<FileRecvSummary, FileSyncError> {
        let ack = self.sync(&mut conn).await.map_err(|e| {
            LOGGER.error(format!("FileSync failed: {:?}", e));
            FileSyncError::Timeout
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.enc_tmp_path)
            .await
            .map_err(|e| {
                LOGGER.error(format!(
                    "Failed to open target file {}: {:?}",
                    self.target_path.display(),
                    e
                ));
                FileSyncError::SystemError
            })?;

        let start_download_time = std::time::Instant::now();
        let f_sz = self
            .download_to_file(conn, &mut file, ack.file_size())
            .await
            .map_err(|e| {
                LOGGER.error(format!("Failed to download file: {:?}", e));
                FileSyncError::AbortedByPeer
            })?;

        if let Err(e) = file.flush().await {
            LOGGER.error(format!("Failed to flush file: {:?}", e));
            return Err(FileSyncError::SystemError);
        }

        let start_decrypt_time = std::time::Instant::now();
        let passphrase = format!("{}", ack.nonce());
        f_from_encryption(&self.enc_tmp_path, &self.target_path, &passphrase)
            .await
            .map_err(|e| {
                LOGGER.error(format!(
                    "Failed to decrypt file: {:?}, {:?}",
                    &self.enc_tmp_path, e
                ));
                FileSyncError::FileMalformed
            })?;

        let summary = FileRecvSummary::new(
            ack.nonce(),
            f_sz,
            self.target_path.clone(),
            start_download_time.elapsed(),
            start_decrypt_time.elapsed(),
        );
        Ok(summary)
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
