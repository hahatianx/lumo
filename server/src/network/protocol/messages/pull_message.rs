use api_model::protocol::protocol::Protocol;
use std::time::SystemTime;

pub struct PullRequest {
    path: String,
    challenge: u32,
    time_stamp: SystemTime,
}

impl PullRequest {
    pub fn new(path: String, challenge: u32) -> Self {
        Self {
            path,
            challenge,
            time_stamp: SystemTime::now(),
        }
    }
}

pub struct PullMessage {}
