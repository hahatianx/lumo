use crate::err::Result;
use crate::global_var::LOGGER;
use crate::tasks::handlers::AsyncHandleable;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A simple async task queue backed by Tokio mpsc that carries Protocol messages.
/// Other components can enqueue Box<dyn Protocol + Send> for handling by a
/// single background consumer task.
#[derive(Debug)]
pub struct TaskQueue {
    tx: mpsc::Sender<QueueMsg>,
    worker: JoinHandle<()>,
}

/// A cloneable, thread-safe sending handle that can be shared across threads.
#[derive(Clone)]
pub struct TaskQueueSender {
    tx: mpsc::Sender<QueueMsg>,
}

impl TaskQueueSender {
    /// Async send that applies backpressure if the channel is full.
    /// Note: We require 'static here because items are ultimately handled on a
    /// dedicated OS thread (see dispatch). The thread may outlive the caller's
    /// stack frame, so messages cannot hold non-'static borrows.
    pub async fn send(&self, msg: Box<AsyncHandleable>) -> Result<()> {
        if let Err(_e) = self.tx.send(QueueMsg::Item(msg)).await {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "TaskQueue worker task is not running",
            )
            .into());
        }
        Ok(())
    }

    /// Non-async, immediate send attempt that can be called from any thread/context.
    /// Returns an error if the channel is full or closed.
    /// Note: Same 'static rationale as send(): items cross a thread boundary.
    pub fn try_send(&self, msg: Box<AsyncHandleable>) -> Result<()> {
        match self.tx.try_send(QueueMsg::Item(msg)) {
            Ok(_) => Ok(()),
            Err(e) => {
                use std::io::{Error, ErrorKind};
                let kind = match e {
                    tokio::sync::mpsc::error::TrySendError::Full(_) => ErrorKind::WouldBlock,
                    tokio::sync::mpsc::error::TrySendError::Closed(_) => ErrorKind::BrokenPipe,
                };
                Err(Error::new(kind, "TaskQueue sender failed to enqueue").into())
            }
        }
    }
}

/// Configuration for TaskQueue
#[derive(Clone, Debug)]
pub struct TaskQueueConfig {
    /// Max queued messages before backpressure. If 0, a very large bound is used.
    pub queue_bound: usize,
}

impl Default for TaskQueueConfig {
    fn default() -> Self {
        Self { queue_bound: 1024 }
    }
}

/// Internal message type for the queue
// The queue carries boxed messages that must be 'static because they will be
// moved into an OS thread via dispatch(). Using 'static here makes the API
// intent explicit and prevents enqueuing messages that borrow short‑lived data.
enum QueueMsg {
    Item(Box<AsyncHandleable>),
    Shutdown,
}

impl TaskQueue {
    /// Create a new TaskQueue and spawn the consumer.
    pub fn new(config: TaskQueueConfig) -> Self {
        let (tx, mut rx) = if config.queue_bound == 0 {
            mpsc::channel(usize::MAX / 2)
        } else {
            mpsc::channel(config.queue_bound)
        };

        let worker = tokio::spawn(async move {
            // Consumer loop: receive messages and handle them.
            while let Some(msg) = rx.recv().await {
                match msg {
                    QueueMsg::Item(item) => {
                        // Dispatch each message to its own thread to handle.
                        Self::dispatch(item);
                    }
                    QueueMsg::Shutdown => {
                        break;
                    }
                }
            }
        });

        Self { tx, worker }
    }

    fn dispatch(mut msg: Box<AsyncHandleable>) {
        // We hop from the Tokio task onto an OS thread here. std::thread::spawn
        // requires the closure (and everything it moves) to be 'static because the
        // spawned thread can outlive the caller's stack frame. By taking a Box<dyn
        // Handleable + Send + 'static>, we guarantee the message carries no
        // borrows.
        let _ = std::thread::spawn(move || {
            // Call the message handler in its own thread; log errors.
            match msg.handle() {
                Ok(()) => {}
                Err(e) => {
                    LOGGER.error(format!("An error occurred while handling message {:?}", e));
                }
            }
        });
    }

    /// Get a cloneable sender handle that can be shared across threads.
    pub fn sender(&self) -> TaskQueueSender {
        TaskQueueSender {
            tx: self.tx.clone(),
        }
    }

    /// Gracefully shutdown the queue by sending a Shutdown message and awaiting the worker task.
    pub async fn shutdown(self) -> Result<()> {
        let _ = self.tx.send(QueueMsg::Shutdown).await;
        let _ = self.worker.await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::protocol::Protocol;

    // A simple test Protocol implementation for verifying the queue behavior.
    #[derive(Clone, Debug)]
    struct TestProto(&'static str);

    impl Protocol for TestProto {
        fn serialize(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }
        fn deserialize(_bytes: &[u8]) -> Result<Self>
        where
            Self: Sized,
        {
            Ok(TestProto("x"))
        }
        fn from_tokens(_tokens: &[crate::network::protocol::token::Token]) -> Result<Self>
        where
            Self: Sized,
        {
            Ok(TestProto("y"))
        }
    }

    impl crate::tasks::handlers::Handleable for TestProto {
        fn handle(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn task_queue_accepts_and_processes() -> Result<()> {
        let q = TaskQueue::new(TaskQueueConfig { queue_bound: 8 });
        let sender = q.sender();
        // Enqueue a few messages
        sender.send(Box::new(TestProto("a"))).await?;
        sender.send(Box::new(TestProto("b"))).await?;
        sender.send(Box::new(TestProto("c"))).await?;

        // Also test cross-thread try_send via sender handle
        let handle = q.sender();
        std::thread::spawn(move || {
            let _ = handle.try_send(Box::new(TestProto("from-thread")));
        })
        .join()
        .unwrap();

        // Shutdown gracefully
        q.shutdown().await?;
        Ok(())
    }
}
