mod handlers;
pub use handlers::AsyncHandleable;
mod helpers;
mod jobs;
pub mod task_queue;

use crate::core::tasks::task_queue::TaskQueue;
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
