use std::time::SystemTime;

type Checksum = u64;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalFile {
    pub key: String,
    pub path: String,

    pub size: u64,
    pub last_write: String,
    pub checksum: Checksum,
    pub last_modified: SystemTime,

    pub is_active: bool,
    pub is_stale: bool,
}
