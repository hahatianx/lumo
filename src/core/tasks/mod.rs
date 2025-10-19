mod handlers;
pub use handlers::AsyncHandleable;
mod job_summary;
use crate::core::tasks::jobs::{
    get_first_hello_message_closure, get_job_heartbeat_closure, job_fs_stale_rescan,
    job_peer_table_anti_entropy, launch_oneshot_job, launch_periodic_job,
};

mod jobs;
mod low_level_tasks;
pub mod task_queue;

use crate::core::tasks::task_queue::{TaskQueue, TaskQueueSender};
use crate::err::Result;

pub async fn init_task_queue() -> Result<TaskQueue> {
    Ok(TaskQueue::new(task_queue::TaskQueueConfig {
        queue_bound: 1024,
    }))
}

pub async fn shutdown_core(task_queue: TaskQueue) -> Result<()> {
    task_queue.shutdown().await?;
    Ok(())
}

pub async fn init_jobs(sender: &TaskQueueSender) -> Result<()> {
    let peer_table_anti_entropy_job = launch_periodic_job(
        "Peer table anti-entropy",
        "Scans and invalidates expired peers in a periodic fashion",
        job_peer_table_anti_entropy,
        60,
        sender.clone(),
    )
    .await?;

    let first_hello_message_job = launch_oneshot_job(
        "Server merged into network",
        "Send out the first HelloMessage and receive response",
        get_first_hello_message_closure(sender).await?,
        Some(30),
        sender.clone(),
    )
    .await?;

    let heartbeat_job = launch_periodic_job(
        "Heartbeat",
        "Periodically sends HelloMessage to inactive itself in neighbors' peer tables",
        get_job_heartbeat_closure(sender).await?,
        30,
        sender.clone(),
    )
    .await?;

    let fs_stable_rescan_job = launch_periodic_job(
        "Stale job rescan",
        "Periodically rescans stale job records from index and updates indices",
        job_fs_stale_rescan,
        60,
        sender.clone(),
    )
    .await?;

    let fs_inactive_cleanup_job = launch_periodic_job(
        "Inactive job cleanup",
        "Periodically cleans up inactive job records from index and updates indices",
        jobs::job_fs_inactive_cleanup,
        60,
        sender.clone(),
    )
    .await?;

    job_summary::JOB_TABLE.print_jobs().await?;

    Ok(())
}

pub async fn shutdown_jobs() -> Result<()> {
    Ok(())
}
