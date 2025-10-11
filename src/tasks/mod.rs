mod handlers;
pub use handlers::Handleable;
pub mod task_queue;

use crate::err::Result;
use crate::tasks::task_queue::TaskQueue;

pub async fn init_core() -> Result<TaskQueue> {
    Ok(TaskQueue::new(task_queue::TaskQueueConfig { queue_bound: 1024 }))
}

pub async fn shutdown_core(task_queue: TaskQueue) -> Result<()> {
    task_queue.shutdown().await?;
    Ok(())
}
