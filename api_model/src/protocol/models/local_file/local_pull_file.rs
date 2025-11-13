use serde::{Deserialize, Serialize};

type Checksum = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PullFileError {
    FileOutdated = 400,
    FileInvalid = 401,
    AccessDenied = 403,
    FileNotFound = 404,
    InternalError = 500,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocalPullFileResult {
    Accept(u64),
    Reject(PullFileError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPullFileRequest {
    pub path: String,
    pub expected_checksum: Option<Checksum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPullFileResponse {
    pub result: LocalPullFileResult,
}
