type Checksum = u64;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PullFileRequest {
    pub peer_identifier: String,
    pub path: String,
    pub expected_checksum: Option<Checksum>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PullFileResponse;
