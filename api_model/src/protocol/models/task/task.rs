use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    TimedOut,
    Pending,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum JobType {
    Periodic,
    OneTime,
    Claimable,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub job_id: u64,
    pub job_name: String,
    pub summary: String,

    pub launch_time: SystemTime,
    pub complete_time: Option<SystemTime>,

    pub status: JobStatus,
    pub status_message: Option<String>,

    pub job_type: JobType,

    pub period: Option<u64>,
}
