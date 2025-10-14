use crate::core::tasks::AsyncHandleable;
use crate::core::tasks::job_summary::{JOB_TABLE, JobStatus, JobSummary, JobType};
use crate::core::tasks::jobs::CallbackFunction;
use crate::core::tasks::task_queue::TaskQueueSender;
use crate::err::Result;
use crate::global_var::LOGGER;
use async_trait::async_trait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Explicit outcome for a one-shot job execution
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum OneshotJobResult {
    Success,
    Failure(String),
    TimedOut,
}

pub struct OneshotJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    job_name: String,
    job: J,
    timeout_in_seconds: u64, // 0 means no timeout

    callback: Option<Box<CallbackFunction>>,
}

impl<J, F> OneshotJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    pub fn new(job_name: String, job: J) -> Self {
        Self {
            job_name,
            job,
            timeout_in_seconds: 0,
            callback: None,
        }
    }

    pub fn with_maybe_timeout(mut self, timeout_in_seconds: Option<u64>) -> Self {
        self.timeout_in_seconds = timeout_in_seconds.unwrap_or(0);
        self
    }

    pub fn update_callback(&mut self, callback: Box<CallbackFunction>) {
        self.callback = Some(callback);
    }

    /// Execute the job once with optional timeout semantics.
    /// - If timeout is 0, no timeout is applied.
    /// - If timeout is non-zero, apply tokio::time::timeout.
    /// Returns explicit Success, Failure, or TimedOut.
    async fn execute_job(&mut self) -> OneshotJobResult {
        let fut = (self.job)();

        let result: Result<()> = if self.timeout_in_seconds == 0 {
            fut.await
        } else {
            match tokio::time::timeout(Duration::from_secs(self.timeout_in_seconds), fut).await {
                Ok(inner) => inner,
                Err(_elapsed) => return OneshotJobResult::TimedOut,
            }
        };

        match result {
            Ok(_) => OneshotJobResult::Success,
            Err(err) => {
                let error_msg = format!("Job {} failed with error: {:?}", &self.job_name, err);
                LOGGER.error(&error_msg);
                OneshotJobResult::Failure(error_msg)
            }
        }
    }
}

#[async_trait]
impl<J, F> AsyncHandleable for OneshotJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    async fn handle(&mut self) -> Result<()> {
        if self.callback.is_none() {
            return Err("No callback function provided for oneshot job".into());
        }
        // Placeholder: In the future, we may record the outcome and invoke callbacks.
        // For now, simply execute the job to honor timeout semantics and ignore the result.
        match self.execute_job().await {
            OneshotJobResult::Success => {
                self.callback.as_mut().unwrap()(JobStatus::Completed, String::new()).await?;
            }
            OneshotJobResult::Failure(failure_msg) => {
                self.callback.as_mut().unwrap()(JobStatus::Failed, failure_msg).await?;
            }
            OneshotJobResult::TimedOut => {
                self.callback.as_mut().unwrap()(JobStatus::TimedOut, String::new()).await?;
            }
        }
        Ok(())
    }
}

fn generate_callback_closure(_job_summary: Arc<RwLock<JobSummary>>) -> Box<CallbackFunction> {
    Box::new(move |_job_status, _job_name| {
        let job_summary_clone = _job_summary.clone();
        Box::pin(async move {
            {
                let mut job_summary_guard = job_summary_clone.write().await;
                job_summary_guard.end_job(_job_status, _job_name).await?;
            }
            Ok(())
        })
    })
}

pub async fn launch_oneshot_job<J, F>(
    job_name: &str,
    summary: &str,
    job: J,
    timeout_in_seconds: Option<u64>,
    task_queue_sender: TaskQueueSender,
) -> Result<u32>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut job =
        OneshotJob::new(String::from(job_name), job).with_maybe_timeout(timeout_in_seconds);

    let job_summary = JobSummary::new(
        String::from(job_name),
        String::from(summary),
        JobType::OneTime,
        Some(chrono::Duration::seconds(
            timeout_in_seconds.unwrap_or(0) as i64
        )),
        shutdown_tx,
    );

    let job_idx = JOB_TABLE.insert_job(job_summary).await?;
    let job_detail = JOB_TABLE.get_job(job_idx).await?;
    job.update_callback(generate_callback_closure(job_detail));
    task_queue_sender.send(Box::new(job)).await?;

    Ok(job_idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tasks::job_summary::{JOB_TABLE, JobStatus};
    use crate::core::tasks::task_queue::{TaskQueue, TaskQueueConfig};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn execute_job_success_no_timeout() {
        let mut job = OneshotJob::new("ok_job".to_string(), || async { Ok(()) });
        let res = job.execute_job().await;
        assert_eq!(res, OneshotJobResult::Success);
    }

    #[tokio::test]
    async fn execute_job_failure_no_timeout() {
        let mut job = OneshotJob::new("fail_job".to_string(), || async {
            Err::<(), crate::err::Error>(
                std::io::Error::new(std::io::ErrorKind::Other, "boom").into(),
            )
        });
        let res = job.execute_job().await;
        match res {
            OneshotJobResult::Failure(msg) => {
                assert!(msg.contains("fail_job"));
                assert!(msg.to_lowercase().contains("failed"));
            }
            _ => panic!("expected Failure variant"),
        }
    }

    #[tokio::test]
    async fn execute_job_times_out() {
        let mut job = OneshotJob::new("sleepy".to_string(), || async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            Ok(())
        })
        .with_maybe_timeout(Some(1));
        let res = job.execute_job().await;
        assert_eq!(res, OneshotJobResult::TimedOut);
    }

    #[tokio::test]
    async fn handle_invokes_callback_success() -> Result<()> {
        let statuses: Arc<RwLock<Vec<JobStatus>>> = Arc::new(RwLock::new(Vec::new()));
        let statuses_cb = statuses.clone();
        let mut job = OneshotJob::new("ok".into(), || async { Ok(()) });
        job.update_callback(Box::new(move |status, _msg| {
            let statuses_cb = statuses_cb.clone();
            Box::pin(async move {
                statuses_cb.write().await.push(status);
                Ok(())
            })
        }));
        job.handle().await?;
        let collected = statuses.read().await.clone();
        assert_eq!(collected, vec![JobStatus::Completed]);
        Ok(())
    }

    #[tokio::test]
    async fn handle_invokes_callback_failure() {
        let statuses: Arc<RwLock<Vec<JobStatus>>> = Arc::new(RwLock::new(Vec::new()));
        let statuses_cb = statuses.clone();
        let mut job = OneshotJob::new("bad".into(), || async {
            Err::<(), crate::err::Error>(
                std::io::Error::new(std::io::ErrorKind::Other, "nope").into(),
            )
        });
        job.update_callback(Box::new(move |status, _msg| {
            let statuses_cb = statuses_cb.clone();
            Box::pin(async move {
                statuses_cb.write().await.push(status);
                Ok(())
            })
        }));
        let _ = job.handle().await;
        let collected = statuses.read().await.clone();
        assert_eq!(collected, vec![JobStatus::Failed]);
    }

    #[tokio::test]
    async fn handle_invokes_callback_timeout() {
        let statuses: Arc<RwLock<Vec<JobStatus>>> = Arc::new(RwLock::new(Vec::new()));
        let statuses_cb = statuses.clone();
        let mut job = OneshotJob::new("slow".into(), || async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            Ok(())
        })
        .with_maybe_timeout(Some(1));
        job.update_callback(Box::new(move |status, _msg| {
            let statuses_cb = statuses_cb.clone();
            Box::pin(async move {
                statuses_cb.write().await.push(status);
                Ok(())
            })
        }));
        let _ = job.handle().await;
        let collected = statuses.read().await.clone();
        assert_eq!(collected, vec![JobStatus::TimedOut]);
    }

    #[tokio::test]
    async fn launch_oneshot_job_integration_marks_job_completed() -> Result<()> {
        let queue = TaskQueue::new(TaskQueueConfig { queue_bound: 8 });
        let sender = queue.sender();

        let idx = launch_oneshot_job(
            "integration_ok",
            "test",
            || async { Ok(()) },
            Some(1),
            sender,
        )
        .await?;

        // Give the queue some time to process the job
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Fetch job and verify it is no longer Running by attempting to end it again
        let job = JOB_TABLE.get_job(idx).await?;
        let mut guard = job.write().await;
        let res = guard
            .end_job(
                JobStatus::Completed,
                String::from("should fail if already ended"),
            )
            .await;
        assert!(res.is_err(), "Job should have already ended");

        queue.shutdown().await?;
        Ok(())
    }
}
