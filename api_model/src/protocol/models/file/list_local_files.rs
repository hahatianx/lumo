use crate::protocol::models::file::local_file::LocalFile;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
// add pagination and filters
pub struct ListLocalFilesRequest;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListLocalFilesResponse {
    pub local_files: Vec<LocalFile>,
}
