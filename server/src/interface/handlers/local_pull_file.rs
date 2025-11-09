use crate::err::Result;
use crate::fs::process_pull_request;
use api_model::protocol::models::local_pull_file::{
    LocalPullFileRequest, LocalPullFileResponse, LocalPullFileResult, PullFileError,
};
use cli_handler::cli_handler;

#[cli_handler(LocalPullFile)]
pub async fn local_pull_file(request: &LocalPullFileRequest) -> Result<LocalPullFileResponse> {
    let file_path = request.path.clone();
    let expected_checksum = request.expected_checksum;

    let result = match process_pull_request(&file_path, expected_checksum).await {
        Ok((nonce, _)) => LocalPullFileResult::Accept(nonce),
        Err(e) => LocalPullFileResult::Reject(PullFileError::FileNotFound),
    };

    Ok(LocalPullFileResponse { result })
}
