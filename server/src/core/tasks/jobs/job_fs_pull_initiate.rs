use crate::core::tasks::jobs::JobClosure;
use crate::core::tasks::low_level_tasks::{SendControlMessageTask, SendType};
use crate::core::topology::Peer;
use crate::err::Result;
use crate::fs::start_file_download_task;
use crate::global_var::get_task_queue_sender;
use crate::network::protocol::messages::PullMessage;
use crate::types::Expected;
use api_model::protocol::protocol::Protocol;
use bytes::Bytes;
use std::path::PathBuf;

type Checksum = u64;

pub async fn get_job_fs_pull_initiate_closure(
    peer: &Peer,
    file_path: &str,
    from_checksum: Expected<Checksum>,
    to_checksum: Expected<Checksum>,
) -> Result<Box<JobClosure>> {
    let target_addr: std::net::SocketAddr = format!("{}:{}", peer.peer_addr.to_string(), crate::constants::UPD_MESSAGE_PORT).parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Invalid peer address: {}, {:?}", e, peer.peer_addr),
        )
    })?;
    let file_path_buf = PathBuf::from(file_path);
    let closure = move || {
        // This is not efficient in terms of memory usage, but it's fine for now.
        // It's better to move to FnOnce(), but it's too late to change the API. Q_Q
        let target_addr = target_addr.clone();
        let file_path_buf = file_path_buf.clone();
        let fut: std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> =
            Box::pin(async move {
                let file_download_challenge =
                    start_file_download_task(&file_path_buf, from_checksum, to_checksum).await?;

                let pull_message = PullMessage::new(
                    file_path_buf.to_str().unwrap(),
                    to_checksum,
                    file_download_challenge,
                )?
                .serialize();

                let send_message_task = SendControlMessageTask::new(
                    SendType::Unicast(target_addr),
                    Bytes::from(pull_message),
                );

                let task_queue = get_task_queue_sender().await?;
                task_queue.send(Box::new(send_message_task)).await?;

                Ok(())
            });
        fut
    };

    Ok(Box::new(closure))
}
