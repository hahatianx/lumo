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
use crate::global_var::{ENV_VAR, LOGGER, get_task_queue_sender};
use rand::random;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::sync::RwLock;

/// Information we keep for a pending pull (awaiting a remote downloader to take over)
pub struct PendingPull {
    pub nonce: u64,
    pub original_path: PathBuf,
    pub temp_path: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub handle: ClaimableJobHandle,
}

/// Global registry nonce -> PendingPull
static PENDING_PULLS: LazyLock<RwLock<HashMap<u64, PendingPull>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub enum RejectionReason {
    PathNotFound,
    PathNotFile,
    FileChecksumMismatch,
}

pub enum PullRequestResult {
    Accept(u64),
    Reject(RejectionReason),
}

/// Core implementation for processing a file pull request.
/// Returns (nonce, temp_path) on success.
pub async fn start_pull_request(
    path_str: &str,
    expected_checksum: Option<u64>,
) -> Result<PullRequestResult> {
    // Resolve and validate source path
    let base = PathBuf::from(ENV_VAR.get().unwrap().get_working_dir());
    let src = secure_join(&base, Path::new(path_str))?;

    if !src.exists() {
        return Ok(PullRequestResult::Reject(RejectionReason::PathNotFound));
    }

    // Basic existence check
    let meta = tokio::fs::metadata(&src).await?;
    if !meta.is_file() {
        return Ok(PullRequestResult::Reject(RejectionReason::PathNotFile));
    }

    let nonce = random::<u64>();
    let file_name = src
        .file_name()
        .ok_or_else(|| format!("Invalid file name for {}", src.display()))?;

    // Prepare temp destination in .disc/tmp_downloads
    let tmp_dir = base
        .join(".disc")
        .join("tmp_downloads")
        .join(format!("{:x}", nonce));
    tokio::fs::create_dir_all(&tmp_dir).await?; // best-effort

    let tmp_dest = tmp_dir.join(format!("{}.{}", file_name.to_string_lossy(), nonce));

    // Copy the file to temp for exclusive transfer
    let found_checksum = {
        LOGGER.debug(format!(
            "Copying file '{}' to temp location '{}'",
            src.display(),
            tmp_dest.display()
        ));

        match fs_lock::RwLock::new(&src).read().await {
            Ok(_) => {}
            Err(e) => {
                return Ok(PullRequestResult::Reject(RejectionReason::PathNotFound));
            }
        }
        LOGGER.debug(format!("read guard fetched {}", src.display()));

        let lumo_file = crate::fs::file::LumoFile::new(src.clone()).await?;
        let checksum = lumo_file.get_checksum().await?;
        tokio::fs::copy(&src, &tmp_dest).await?;
        LOGGER.debug(format!(
            "Copied file '{}' to temp location '{}', file checksum {}",
            src.display(),
            tmp_dest.display(),
            checksum
        ));

        checksum
    };

    if expected_checksum.is_some() && found_checksum != expected_checksum.unwrap() {
        return Ok(PullRequestResult::Reject(
            RejectionReason::FileChecksumMismatch,
        ));
    }

    // Launch a claimable job representing this pending transfer
    let q_sender = get_task_queue_sender().await?;
    let job_name = format!("pull:{}", file_name.to_string_lossy());
    let summary = format!(
        "Pending file transfer for {} (nonce={:x})",
        file_name.to_string_lossy(),
        nonce
    );
    let cleanup = move || {
        let tmp_dir_for_cleanup = tmp_dir.clone();
        async move {
            // If the job times out without being claimed, try to remove the temp file
            if let Err(e) = tokio::fs::remove_dir_all(&tmp_dir_for_cleanup).await {
                LOGGER.error(format!(
                    "Failed to remove temp dir {} (err: {:?})",
                    &tmp_dir_for_cleanup.display(),
                    e
                ));
            }
            Ok(())
        }
    };

    let handle = launch_claimable_job(&job_name, &summary, cleanup, 120, q_sender).await?;

    // Insert into registry
    let pending = PendingPull {
        nonce,
        original_path: src.clone(),
        temp_path: tmp_dest.clone(),
        created_at: chrono::Utc::now(),
        handle,
    };
    PENDING_PULLS.write().await.insert(nonce, pending);

    LOGGER.info(format!(
        "Prepared file '{}' for pull, moved to '{}' with nonce {:x}",
        src.display(),
        tmp_dest.display(),
        nonce
    ));

    Ok(PullRequestResult::Accept(nonce))
}

/// Claim a pending transfer by its nonce. Returns the ClaimableJobHandle if present,
/// and removes the entry from the registry. The caller can then call `take_over()` on the handle.
pub async fn claim_by_nonce(nonce: u64) -> Option<PendingPull> {
    PENDING_PULLS.write().await.remove(&nonce)
}

/// Cancel a pending transfer and remove temp file if present.
pub async fn cancel_pending(nonce: u64) {
    let _ = PENDING_PULLS.write().await.remove(&nonce);
}

/// Join `base` with `rel` and ensure the resulting canonical path stays inside `base`.
fn secure_join(base: &Path, rel: &Path) -> Result<PathBuf> {
    let joined = base.join(rel);
    let canon_base = std::fs::canonicalize(base)?;
    let canon_joined = std::fs::canonicalize(&joined)?;
    if !canon_joined.starts_with(&canon_base) {
        return Err(format!(
            "Path traversal detected: '{}' escapes base '{}'",
            canon_joined.display(),
            canon_base.display()
        )
        .into());
    }
    Ok(canon_joined)
}
