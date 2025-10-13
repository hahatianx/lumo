use crate::core::tasks::jobs::{job_peer_table_anti_entropy, launch_periodic_job};
use crate::core::tasks::task_queue::TaskQueue;
use crate::err::Result;
use crate::global_var::LOGGER;
use std::cmp::PartialEq;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

pub static JOB_TABLE: LazyLock<JobTable> = LazyLock::new(|| JobTable::new());

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JobStatus {
    Running = 0,
    Completed,
    Failed,
    TimedOut,
    Shutdown,
}

#[derive(Debug, Copy, Clone)]
pub enum JobType {
    Periodic,
    OneTime,
}

pub struct JobSummary {
    job_name: String,
    launched_time: chrono::DateTime<chrono::Utc>,
    complete_time: Option<chrono::DateTime<chrono::Utc>>,

    status: JobStatus,
    status_msg: Option<String>,

    job_type: JobType,
    period: Option<chrono::Duration>,
    summary: String,

    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl Debug for JobSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            " {{ JobSummary job_name: {}, launched_time: {} }}",
            &self.job_name, &self.launched_time
        )
    }
}

impl JobSummary {
    pub fn new(
        job_name: String,
        summary: String,
        job_type: JobType,
        period: Option<chrono::Duration>,
        shutdown_tx: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        Self {
            job_name,
            launched_time: chrono::Utc::now(),
            complete_time: None,
            status: JobStatus::Running,
            status_msg: None,
            job_type,
            period,
            summary,
            shutdown_tx,
        }
    }

    pub async fn update_status(&mut self, new_status: JobStatus) {
        self.status = new_status;
    }

    pub async fn end_job(&mut self, status: JobStatus) -> Result<()> {
        if self.status != JobStatus::Running {
            let error_msg = format!(
                "Failed to end job {} because it's not running. Status found: {:?}",
                &self.job_name, &self.status
            );
            LOGGER.error(&error_msg);
            return Err(error_msg.into());
        }
        let end_time = chrono::Utc::now();
        self.complete_time = Some(end_time);
        self.status = status;
        Ok(())
    }

    pub async fn update_status_msg(&mut self, status_msg: String) {
        self.status_msg = Some(status_msg);
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.end_job(JobStatus::Shutdown).await?;
        let _ = self.shutdown_tx.send(());
        Ok(())
    }
}

pub struct JobTable {
    jobs: RwLock<Vec<Arc<JobSummary>>>,
}

impl JobTable {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(Vec::new()),
        }
    }

    pub async fn insert_job(&self, job: JobSummary) -> Result<u32> {
        let ar_job = Arc::new(job);
        let mut table = self.jobs.write().await;
        table.push(ar_job.clone());
        let job_idx = table.len() as u32 - 1;
        Ok(job_idx)
    }

    pub async fn print_jobs(&self) -> Result<()> {
        let table = self.jobs.read().await;
        for job in table.iter() {
            eprintln!("{:?}", &job);
        }
        Ok(())
    }
}
