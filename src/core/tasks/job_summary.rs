use crate::err::Result;
use crate::global_var::LOGGER;
use std::cmp::PartialEq;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

pub static JOB_TABLE: LazyLock<JobTable> = LazyLock::new(|| JobTable::new());

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JobStatus {
    Pending = 0,
    Running,
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

    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Debug for JobSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            " {{ JobSummary job_name: {}, launched_time: {}, complete_time: {:?}, job_status: {:?}, msg: {:?} }}\n",
            &self.job_name, &self.launched_time, &self.complete_time, &self.status, &self.status_msg
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
            shutdown_tx: Some(shutdown_tx),
        }
    }

    pub async fn update_status(&mut self, new_status: JobStatus) {
        self.status = new_status;
    }

    pub async fn end_job(&mut self, status: JobStatus, status_msg: String) -> Result<()> {
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
        self.status_msg = Some(status_msg);
        Ok(())
    }

    pub async fn update_status_msg(&mut self, status_msg: String) {
        self.status_msg = Some(status_msg);
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.end_job(JobStatus::Shutdown, String::from("Shutdown by system"))
            .await?;
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        Ok(())
    }
}

pub struct JobTable {
    jobs: RwLock<Vec<Arc<RwLock<JobSummary>>>>,
}

impl Debug for JobTable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {

        match &self.jobs.try_read() {
            Ok(jobs) => {
                for job in jobs.iter() {
                    match job.try_read() {
                        Ok(job) => {
                            let _ = write!(f, "{:?}", &job);
                        }
                        Err(_e) => {
                            let _ = write!(f, "<Locked>");
                        }
                    }
                }
                Ok(())
            }
            Err(_e) => {
                write!(f, "<Locked>")
            }
        }

    }
}

impl JobTable {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(Vec::new()),
        }
    }

    pub async fn insert_job(&self, job: JobSummary) -> Result<u32> {
        let ar_job = Arc::new(RwLock::new(job));
        {
            let mut table = self.jobs.write().await;
            table.push(ar_job.clone());
            let job_idx = table.len() as u32 - 1;
            Ok(job_idx)
        }
    }

    pub async fn get_job(&self, job_idx: u32) -> Result<Arc<RwLock<JobSummary>>> {
        let table = self.jobs.read().await;
        if job_idx >= table.len() as u32 {
            return Err(format!("Job index {} is out of range", job_idx).into());
        }
        Ok(table[job_idx as usize].clone())
    }

    pub async fn print_jobs(&self) -> Result<()> {
        let table = self.jobs.read().await;
        for job in table.iter() {
            eprintln!("{:?}", &job);
        }
        Ok(())
    }
}
