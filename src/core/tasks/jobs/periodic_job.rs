use crate::core::tasks::AsyncHandleable;
use crate::core::tasks::job_summary::{JobSummary, JobType};
use crate::core::tasks::task_queue::TaskQueueSender;
use crate::err::Result;
use crate::global_var::LOGGER;
use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;
use tokio::select;

/// A periodic async job wrapper that repeatedly runs an async function and sleeps between runs.
pub struct PeriodicJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    job_name: String,
    job: J,
    period_in_seconds: u64,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
}

impl<J, F> PeriodicJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    pub fn new(
        job_name: String,
        job: J,
        period_in_seconds: u64,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Self {
        Self {
            job_name,
            job,
            period_in_seconds,
            shutdown_rx,
        }
    }
}

#[async_trait]
impl<J, F> AsyncHandleable for PeriodicJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    async fn handle(&mut self) -> Result<()> {
        loop {
            // Do not
            match (self.job)().await {
                Ok(()) => {
                    LOGGER.info(format!("Job {} completed successfully.", &self.job_name));
                }
                Err(job_err) => {
                    // We don't want a single job execution failure to crash the periodic job runs.
                    LOGGER.error(format!("Job {} failed: {}", &self.job_name, job_err));
                }
            }
            select! {
                biased;
                _ = &mut self.shutdown_rx => {
                    LOGGER.info(format!("Received a shutdown signal. The job {} will exit.", &self.job_name));
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(self.period_in_seconds)) => {}
            }
        }
        Ok(())
    }
}

pub async fn launch_periodic_job<J, F>(
    job_name: &str,
    summary: &str,
    job: J,
    period_in_seconds: u64,
    task_queue_sender: TaskQueueSender,
) -> Result<JobSummary>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let job = PeriodicJob::new(String::from(job_name), job, period_in_seconds, shutdown_rx);
    task_queue_sender.send(Box::new(job)).await?;

    let period = Some(chrono::Duration::seconds(period_in_seconds as i64));

    Ok(JobSummary::new(
        String::from(job_name),
        String::from(summary),
        JobType::Periodic,
        period,
        shutdown_tx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tasks::task_queue::{TaskQueue, TaskQueueConfig};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn periodic_job_handle_runs_and_shutdowns() -> Result<()> {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let job = move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        };

        let mut periodic = PeriodicJob::new("test-job".to_string(), job, 0, rx);

        // Run in the background
        let handle = tokio::spawn(async move { periodic.handle().await });

        // Allow a short time for multiple iterations
        tokio::time::sleep(Duration::from_millis(30)).await;
        let runs = counter.load(Ordering::SeqCst);
        assert!(runs >= 1, "expected at least one run, got {}", runs);

        // Trigger shutdown and ensure handle completes
        let _ = tx.send(());
        let res = handle.await.expect("join should succeed");
        assert!(res.is_ok(), "handler should return Ok, got {:?}", res);
        Ok(())
    }

    #[tokio::test]
    async fn launch_periodic_job_integration_with_task_queue() -> Result<()> {
        let q = TaskQueue::new(TaskQueueConfig { queue_bound: 8 });
        let sender = q.sender();

        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let job = move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        };

        let summary = launch_periodic_job(
            "integration-job",
            "periodic integration test",
            job,
            0,
            sender,
        )
        .await?;

        // Let it run a bit
        tokio::time::sleep(Duration::from_millis(40)).await;
        let runs = counter.load(Ordering::SeqCst);
        assert!(
            runs >= 1,
            "expected at least one run in queue, got {}",
            runs
        );

        // Shutdown the periodic job via returned summary and then the queue
        summary.shutdown().await?;
        q.shutdown().await?;
        Ok(())
    }
}
