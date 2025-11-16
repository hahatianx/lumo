use crate::core::PEER_TABLE;
use crate::core::tasks::{get_job_fs_pull_initiate_closure, launch_oneshot_job};
use crate::err::Result;
use crate::fs::FS_INDEX;
use crate::global_var::get_task_queue_sender;
use api_model::protocol::models::file::pull_file::{PullFileRequest, PullFileResponse};
use cli_handler::cli_handler;

#[cli_handler(PullFile)]
pub async fn pull_file(request: &PullFileRequest) -> Result<PullFileResponse> {
    let file_path = request.path.clone();
    let expected_checksum = request.expected_checksum;

    // 1. If caller provided an expected checksum, fetch the latest checksum from FS_INDEX
    let from_checksum = FS_INDEX.get_latest_checksum(&file_path).await?;

    // 2. find the peer
    let peer = PEER_TABLE
        .get_peer(&request.peer_identifier)
        .await
        .ok_or_else(|| format!("Peer {} not found", request.peer_identifier))?;

    // 3. initiate an oneshot job get_fs_pull_initiate (to be implemented)
    let task_sender = get_task_queue_sender().await?;
    let _ = launch_oneshot_job(
        "Pull file initiation",
        &format!("Initiate pulling file {} from {}", &file_path, ""),
        get_job_fs_pull_initiate_closure(
            &peer,
            &file_path,
            from_checksum.into(),
            expected_checksum.into(),
        )
        .await?,
        Some(30),
        task_sender,
    )
    .await?;

    Ok(PullFileResponse)
}
