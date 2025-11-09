use crate::protocol::models::task::task::Task;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListTasksRequest;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListTasksResponse {
    pub tasks: Vec<Task>,
}
