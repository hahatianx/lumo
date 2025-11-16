use std::sync::Arc;
// This module defines a job type that itself performs no work. Instead, it waits
// for external asynchronous components (e.g., schedulers, workers, or other tasks)
// to attach/assign real actions. If no action takes over before a configured
// timeout elapses, the job is considered timed out and the provided callback is
// invoked with `JobStatus::TimedOut`.
use crate::core::tasks::AsyncHandleable;
use crate::core::tasks::job_summary::{JOB_TABLE, JobStatus, JobSummary, JobType};
use crate::core::tasks::jobs::JobSummaryStatusCallback;
use crate::core::tasks::task_queue::TaskQueueSender;
use crate::err::Result;
use crate::global_var::LOGGER;
use async_trait::async_trait;
use tokio::select;
use tokio::sync::RwLock;

/// A placeholder job that waits to be taken over by an external actor.
///
/// This job intentionally contains no built-in actions. It exists as a shell
/// that external async tasks can observe and "take over" (attach real work).
/// If no takeover occurs within `timeout_in_seconds`, the job times out and
/// its callback is invoked with `JobStatus::TimedOut`.
pub struct ClaimableJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    /// Human-readable name for diagnostics/metrics.
    job_name: String,

    take_over_indicator: tokio::sync::oneshot::Receiver<()>,

    /// the sender to pass over ownership of job summary callback to the actor.
    take_over_callback: Option<tokio::sync::oneshot::Sender<Option<Box<JobSummaryStatusCallback>>>>,

    /// When the job times out, call clean_up_closure
    clean_up_closure: J,

    /// Maximum time in seconds that the job will wait for a takeover before
    /// it triggers a timeout.
    timeout_in_seconds: u64,

    /// Callback is to update job summary in job table.
    /// On actors taking over, callback is handed over to actors.
    callback: Option<Box<JobSummaryStatusCallback>>,
}

impl<J, F> ClaimableJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    async fn handle_job_timeout(&mut self) -> Result<()> {
        // Timeout occurred before any actor took over. Mark as timed out
        // and notify potential waiters that there is no callback to claim.
        LOGGER.debug(format!(
            "Job {} expired after {} seconds",
            self.job_name, self.timeout_in_seconds
        ));
        self.callback.as_mut().unwrap()(
            JobStatus::TimedOut,
            format!("Job expired after {} seconds", self.timeout_in_seconds),
        )
        .await?;
        if let Some(sender) = self.take_over_callback.take() {
            let _ = sender.send(None);
        }
        (self.clean_up_closure)().await?;
        Ok(())
    }

    async fn handle_job_takeover(&mut self) -> Result<()> {
        // An external actor has claimed the job. Mark as running and
        // transfer ownership of the callback to the actor.
        LOGGER.debug(format!("Job {} claimed by actor", self.job_name));
        self.callback.as_mut().unwrap()(JobStatus::Running, String::new()).await?;
        if let Some(sender) = self.take_over_callback.take() {
            let _ = sender.send(self.callback.take());
        }
        Ok(())
    }
}

#[async_trait]
impl<J, F> AsyncHandleable for ClaimableJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    /// Handles the job by waiting for a takeover until the configured timeout.
    ///
    /// Current behavior:
    /// - Waits for `timeout_in_seconds`.
    /// - If the timeout elapses before a takeover/action occurs, invokes the
    ///   callback with `JobStatus::TimedOut` and a message.
    async fn handle(&mut self) -> Result<()> {
        if self.callback.is_none() {
            return Err("No callback function provided for ClaimableJob".into());
        }
        select! {
            biased;
            result = &mut self.take_over_indicator => {
                match result {
                    Ok(_) => self.handle_job_takeover().await?,
                    _ => self.handle_job_timeout().await?,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(self.timeout_in_seconds)) => self.handle_job_timeout().await?,
        }
        Ok(())
    }
}

impl<J, F> ClaimableJob<J, F>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    pub fn new(
        job_name: &str,
        timeout_in_seconds: u64,
        take_over_receiver: tokio::sync::oneshot::Receiver<()>,
        take_over_callback: tokio::sync::oneshot::Sender<Option<Box<JobSummaryStatusCallback>>>,
        clean_up_closure: J,
    ) -> Self {
        Self {
            job_name: String::from(job_name),
            take_over_indicator: take_over_receiver,
            take_over_callback: Some(take_over_callback),
            clean_up_closure,
            timeout_in_seconds,
            callback: None,
        }
    }

    pub fn update_callback(&mut self, callback: Box<JobSummaryStatusCallback>) {
        self.callback = Some(callback);
    }
}

fn generate_callback_closure(
    job_summary: Arc<RwLock<JobSummary>>,
) -> Box<JobSummaryStatusCallback> {
    Box::new(move |job_status, job_status_msg| {
        let job_summary_clone = job_summary.clone();
        Box::pin(async move {
            {
                let mut job_summary_guard = job_summary_clone.write().await;
                if job_status == JobStatus::Running {
                    job_summary_guard.start_job().await?;
                } else {
                    job_summary_guard
                        .end_job(job_status, job_status_msg)
                        .await?;
                }
            }
            Ok(())
        })
    })
}

pub struct ClaimableJobHandle {
    take_over_callback_recv: tokio::sync::oneshot::Receiver<Option<Box<JobSummaryStatusCallback>>>,
    take_over_indicator: tokio::sync::oneshot::Sender<()>,
}

#[derive(Debug)]
pub enum ClaimableJobTakeoverError {
    JobTimeOut,
    JobDropped,
    JobCallbackError,
}

impl ClaimableJobHandle {
    pub fn new(
        recv: tokio::sync::oneshot::Receiver<Option<Box<JobSummaryStatusCallback>>>,
        take_over_indicator: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        Self {
            take_over_callback_recv: recv,
            take_over_indicator,
        }
    }

    pub async fn take_over(
        self,
    ) -> std::result::Result<Box<JobSummaryStatusCallback>, ClaimableJobTakeoverError> {
        self.take_over_indicator
            .send(())
            .map_err(|_| ClaimableJobTakeoverError::JobDropped)?;
        match self.take_over_callback_recv.await {
            Ok(Some(callback)) => Ok(callback),
            Ok(None) => Err(ClaimableJobTakeoverError::JobTimeOut),
            Err(_) => Err(ClaimableJobTakeoverError::JobDropped),
        }
    }
}

pub async fn launch_claimable_job<J, F>(
    job_name: &str,
    summary: &str,
    clean_up_closure: J,
    timeout_in_seconds: u64,
    task_queue_sender: TaskQueueSender,
) -> Result<ClaimableJobHandle>
where
    J: FnMut() -> F + Send + 'static,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let (take_over_indicator, take_over_receiver) = tokio::sync::oneshot::channel::<()>();
    let (take_over_callback_sender, take_over_callback_recv) =
        tokio::sync::oneshot::channel::<Option<Box<JobSummaryStatusCallback>>>();

    let mut job = ClaimableJob::new(
        job_name,
        timeout_in_seconds,
        take_over_receiver,
        take_over_callback_sender,
        clean_up_closure,
    );

    let job_summary = JobSummary::new(
        String::from(job_name),
        String::from(summary),
        JobStatus::Pending,
        JobType::Claimable,
        Some(chrono::Duration::seconds(timeout_in_seconds as i64)),
        None,
    );
    let job_idx = JOB_TABLE.insert_job(job_summary).await?;
    let job_detail = JOB_TABLE.get_job(job_idx).await?;
    job.update_callback(generate_callback_closure(job_detail));
    let job_handle = ClaimableJobHandle::new(take_over_callback_recv, take_over_indicator);
    task_queue_sender.send(Box::new(job)).await?;
    Ok(job_handle)
}
