use crate::err::Result;
use crate::fs::FS_INDEX;
use api_model::protocol::models::file::list_local_files::{
    ListLocalFilesRequest, ListLocalFilesResponse,
};
use api_model::protocol::models::file::local_file::LocalFile;
use cli_handler::cli_handler;

#[cli_handler(ListLocalFiles)]
pub async fn list_local_files(_request: &ListLocalFilesRequest) -> Result<ListLocalFilesResponse> {
    let files = FS_INDEX.dump_all_files().await?;

    let file_list = files
        .into_iter()
        .map(|(path, file)| LocalFile {
            key: path.to_string_lossy().to_string(),
            path: file.path.to_string(),
            size: file.size,
            last_write: file.last_write.to_string(),
            checksum: file.checksum,
            last_modified: file.last_modified,
            is_active: file.is_active,
            is_stale: file.is_stale,
        })
        .collect::<Vec<_>>();

    Ok(ListLocalFilesResponse {
        local_files: file_list,
    })
}
