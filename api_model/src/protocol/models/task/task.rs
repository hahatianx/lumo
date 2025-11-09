use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub job_id: u64,
    pub job_name: String,
    pub summary: String,

    pub launch_time: u64,
    pub complete_time: Option<u64>,

    pub status: String,
    pub status_message: Option<String>,

    pub job_type: String,

    pub period: Option<u64>,
}
