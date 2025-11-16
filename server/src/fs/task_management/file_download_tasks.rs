use crate::core::tasks::{ClaimableJobHandle, launch_claimable_job};
use crate::err::Result;
use crate::global_var::{LOGGER, get_task_queue_sender};
use crate::types::Expected;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::sync::RwLock;

type Checksum = u64;
type Challenge = u64;

pub struct PendingFileDownloadTask {
    pub challenge: u64,

    pub file_path: PathBuf,
    // Download file from remote peer, and replace local file_path from `from_checksum` to `to_checksum` if checksum matches.
    pub from_checksum: Expected<Checksum>,
    pub to_checksum: Expected<Checksum>,

    pub created_at: chrono::DateTime<chrono::Utc>,
    pub handle: Option<ClaimableJobHandle>,
}

impl PendingFileDownloadTask {
    pub fn new(
        challenge: Challenge,
        file_path: PathBuf,
        from_checksum: Expected<Checksum>,
        to_checksum: Expected<Checksum>,
        handle: ClaimableJobHandle,
    ) -> Self {
        Self {
            challenge,
            file_path,
            from_checksum,
            to_checksum,
            created_at: chrono::Utc::now(),
            handle: Some(handle),
        }
    }
}

static PENDING_DOWNLOADS: LazyLock<RwLock<HashMap<Challenge, PendingFileDownloadTask>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

async fn cancel_pending(challenge: Challenge) {
    LOGGER.debug(format!(
        "Removing download task for challenge {} from pending downloads map, result is ignored",
        challenge
    ));
    let _ = PENDING_DOWNLOADS.write().await.remove(&challenge);
}

async fn claim_by_nonce(challenge: Challenge) -> Option<PendingFileDownloadTask> {
    PENDING_DOWNLOADS.write().await.remove(&challenge)
}

pub async fn claim_pending_download(challenge: Challenge) -> Option<PendingFileDownloadTask> {
    claim_by_nonce(challenge).await
}

async fn insert_download_task(task: PendingFileDownloadTask) {
    PENDING_DOWNLOADS.write().await.insert(task.challenge, task);
}

pub async fn start_file_download_task<P: AsRef<Path>>(
    path: P,
    from_checksum: Expected<Checksum>,
    to_checksum: Expected<Checksum>,
) -> Result<Challenge> {
    // Make sure the challenge is not 0, 0 is reserved for bad requests, and the server will not respond to challenges with 0.
    let challenge = rand::rng().random_range(1..u64::MAX);

    let q_sender = get_task_queue_sender().await?;
    let job_name = format!(
        "download:{}, checksum: {}",
        path.as_ref().to_path_buf().display(),
        match to_checksum {
            Expected::Value(c) => c.to_string(),
            Expected::Any => String::from("-"),
        }
    );
    let summary = format!(
        "Pending file download for {}",
        path.as_ref().to_path_buf().display()
    );
    let cleanup = move || async move {
        cancel_pending(challenge).await;
        Ok(())
    };
    let download_job_handle =
        launch_claimable_job(&job_name, &summary, cleanup, 120, q_sender).await?;

    let pending_download_task = PendingFileDownloadTask::new(
        challenge,
        path.as_ref().to_path_buf(),
        from_checksum,
        to_checksum,
        download_job_handle,
    );
    insert_download_task(pending_download_task).await;

    LOGGER.info(format!(
        "Pending file download for {}, checksum {}, with challenge {}",
        path.as_ref().to_path_buf().display(),
        match to_checksum {
            Expected::Value(c) => c.to_string(),
            Expected::Any => String::from("-"),
        },
        challenge
    ));

    Ok(challenge)
}
