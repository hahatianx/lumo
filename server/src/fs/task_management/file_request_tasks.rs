//! Remote async file pull request framework
//!
//! Flow:
//! 1) Peer sends a pull_request message with a file path (relative to working_dir) and optional checksum.
//! 2) Server validates path and (optionally) verifies checksum.
//! 3) Server moves the file into .disc/tmp_downloads, launches a claimable job, and generates a random nonce.
//! 4) Server saves the claimable job handle together with the nonce in a global map so the downloader can "claim" it.

use crate::core::tasks::{ClaimableJobHandle, launch_claimable_job};
use crate::err::Result;
use crate::fs::fs_lock;
use crate::fs::util::normalize_path;
use crate::global_var::{ENV_VAR, LOGGER, get_task_queue_sender};
use crate::types::Expected;
use crate::utilities::crypto::f_to_encryption;
use crate::utilities::temp_dir::TmpDirGuard;
use rand::random;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;

type Checksum = u64;
type Nonce = u64;

/// Information we keep for a pending pull (awaiting a remote downloader to take over)
pub struct PendingPull {
    pub nonce: Nonce,
    pub original_path: PathBuf,
    pub temp_file_path: PathBuf,
    pub temp_path: TmpDirGuard,
    pub checksum: Checksum,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub handle: Option<ClaimableJobHandle>,
}

impl PendingPull {
    async fn create_claimable_job(nonce: Nonce, file_name: &PathBuf) -> Result<ClaimableJobHandle> {
        // Launch a claimable job representing this pending transfer
        let q_sender = get_task_queue_sender().await?;
        let job_name = format!("pull:{}", file_name.to_string_lossy());
        let summary = format!(
            "Pending file transfer for {} (nonce={:x})",
            file_name.to_string_lossy(),
            nonce
        );
        let cleanup = move || async move {
            cancel_pending(nonce).await;
            Ok(())
        };
        Ok(launch_claimable_job(&job_name, &summary, cleanup, 120, q_sender).await?)
    }

    /// source_path is either relative to working_dir or absolute.
    /// Returns a PendingPull on success, or an error if the file cannot be validated or copied.
    pub async fn validate_and_new<P>(
        nonce: Nonce,
        source_path: P,
        expected_checksum: Expected<Checksum>,
    ) -> std::result::Result<Self, RejectionReason>
    where
        P: AsRef<Path>,
    {
        let original_full_path: PathBuf = normalize_path(source_path.as_ref().to_str().unwrap())
            .map_err(|e| {
                LOGGER.error(format!(
                    "Failed to normalize path '{}': {:?}",
                    source_path.as_ref().to_str().unwrap(),
                    e
                ));
                RejectionReason::SystemError
            })?;

        // Prepare temp destination in .disc/tmp_downloads
        let base_download_dir = PathBuf::from(ENV_VAR.get().unwrap().get_temp_downloads_dir());
        let tmp_dir = base_download_dir.join(format!("send-{:x}", nonce));
        tokio::fs::create_dir_all(&tmp_dir).await.map_err(|e| {
            LOGGER.error(format!(
                "Failed to create temp dir {}: {:?}",
                tmp_dir.display(),
                e
            ));
            RejectionReason::SystemError
        })?; // best-effort

        let file_name = original_full_path.file_name().ok_or_else(|| {
            LOGGER.error(format!(
                "Failed to get file name from path '{}'",
                original_full_path.display()
            ));
            RejectionReason::SystemError
        })?;
        let tmp_dest = tmp_dir.join(format!("{}.{}", file_name.to_string_lossy(), nonce));
        // Guard to ensure temp dir is removed if we error out before constructing PendingPull
        let tmp_dir_guard: TmpDirGuard = tmp_dir.into();

        // Copy the file to temp for exclusive transfer
        let found_checksum = {
            LOGGER.debug(format!(
                "Copying file '{}' to temp location '{}'",
                original_full_path.display(),
                tmp_dest.display()
            ));

            let read_guard = fs_lock::RwLock::new(&original_full_path)
                .read()
                .await
                .map_err(|e| {
                    LOGGER.error(format!(
                        "Failed to acquire read lock for {}: {:?}",
                        original_full_path.display(),
                        e
                    ));
                    RejectionReason::PathNotFound
                })?;

            let (f_size, m_time, checksum, read_guard) =
                crate::fs::file::get_file_checksum(read_guard)
                    .await
                    .map_err(|e| {
                        LOGGER.error(format!(
                            "Failed to get checksum for {}: {:?}",
                            original_full_path.display(),
                            e
                        ));
                        RejectionReason::SystemError
                    })?;

            let passphrase = format!("{}", nonce);
            f_to_encryption(&original_full_path, &tmp_dest, &passphrase)
                .await
                .map_err(|e| {
                    LOGGER.error(format!(
                        "Failed to encrypt file {}: {:?}",
                        original_full_path.display(),
                        e
                    ));
                    RejectionReason::SystemError
                })?;

            LOGGER.debug(format!(
                "Copied and encrypted file '{}' to temp location '{}', file checksum {}",
                original_full_path.display(),
                tmp_dest.display(),
                checksum
            ));

            checksum
        };

        if expected_checksum.not_match_expected(&found_checksum) {
            return Err(RejectionReason::FileChecksumMismatch);
        }

        let handle = Self::create_claimable_job(nonce, &original_full_path)
            .await
            .map_err(|e| {
                LOGGER.error(format!(
                    "Failed to create claimable job for {}: {:?}",
                    original_full_path.display(),
                    e
                ));
                RejectionReason::SystemError
            })?;

        Ok(Self {
            nonce,
            original_path: original_full_path,
            temp_file_path: tmp_dest,
            temp_path: tmp_dir_guard,
            checksum: found_checksum,
            created_at: chrono::Utc::now(),
            handle: Some(handle),
        })
    }
}

impl Drop for PendingPull {
    fn drop(&mut self) {
        LOGGER.debug(format!(
            "PendingPull dropped for nonce {:x}, temp_path: {:?}",
            self.nonce, self.temp_path
        ));
    }
}

/// Global registry nonce -> PendingPull
static PENDING_PULLS: LazyLock<RwLock<HashMap<Nonce, PendingPull>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub enum RejectionReason {
    PathNotFound,
    PathNotFile,
    FileChecksumMismatch,
    SystemError,
}

impl Debug for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::PathNotFound => write!(f, "PathNotFound"),
            RejectionReason::PathNotFile => write!(f, "PathNotFile"),
            RejectionReason::FileChecksumMismatch => write!(f, "FileChecksumMismatch"),
            RejectionReason::SystemError => write!(f, "SystemError"),
        }
    }
}

impl Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub enum PullRequestResult {
    Accept(Nonce),
    Reject(RejectionReason),
}

/// Core implementation for processing a file pull request.
/// Returns (nonce, temp_path) on success.
pub async fn start_pull_request(
    path_str: &str,
    expected_checksum: Expected<Checksum>,
) -> Result<PullRequestResult> {
    // Resolve and validate source path
    let src = normalize_path(path_str)?;

    if !src.exists() {
        return Ok(PullRequestResult::Reject(RejectionReason::PathNotFound));
    }

    // Basic existence check
    let meta = tokio::fs::metadata(&src).await?;
    if !meta.is_file() {
        return Ok(PullRequestResult::Reject(RejectionReason::PathNotFile));
    }

    let nonce = random::<u64>();

    match PendingPull::validate_and_new(nonce, src, expected_checksum).await {
        Ok(pending) => {
            PENDING_PULLS.write().await.insert(nonce, pending);
            Ok(PullRequestResult::Accept(nonce))
        }
        Err(e) => Ok(PullRequestResult::Reject(e)),
    }
}

/// Claim a pending transfer by its nonce. Returns the ClaimableJobHandle if present,
/// and removes the entry from the registry. The caller can then call `take_over()` on the handle.
async fn claim_by_nonce(nonce: Nonce) -> Option<PendingPull> {
    PENDING_PULLS.write().await.remove(&nonce)
}

/// Public wrapper to claim a pending pull by nonce.
/// Returns the PendingPull if present and removes it from the registry.
pub async fn claim_pending_pull(nonce: Nonce) -> Option<PendingPull> {
    claim_by_nonce(nonce).await
}

/// Cancel a pending transfer and remove temp file if present.
async fn cancel_pending(nonce: Nonce) {
    LOGGER.debug(format!(
        "Removing pending pull task for nonce {} from pending pulls map, result is ignored",
        nonce
    ));
    let _ = PENDING_PULLS.write().await.remove(&nonce);
}
