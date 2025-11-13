use crate::err::Result;
use crate::fs::{PullRequestResult, RejectionReason, start_pull_request};
use api_model::protocol::models::local_file::local_pull_file::{
    LocalPullFileRequest, LocalPullFileResponse, LocalPullFileResult, PullFileError,
};
use cli_handler::cli_handler;

#[cli_handler(LocalPullFile)]
pub async fn local_pull_file(request: &LocalPullFileRequest) -> Result<LocalPullFileResponse> {
    let file_path = request.path.clone();
    let expected_checksum = request.expected_checksum;

    let result = match start_pull_request(&file_path, expected_checksum.into()).await {
        Ok(PullRequestResult::Accept(nonce)) => LocalPullFileResult::Accept(nonce),
        Ok(PullRequestResult::Reject(reason)) => match reason {
            RejectionReason::PathNotFound => {
                LocalPullFileResult::Reject(PullFileError::FileNotFound)
            }
            RejectionReason::FileChecksumMismatch => {
                LocalPullFileResult::Reject(PullFileError::FileOutdated)
            }
            RejectionReason::PathNotFile => LocalPullFileResult::Reject(PullFileError::FileInvalid),
        },
        Err(e) => LocalPullFileResult::Reject(PullFileError::InternalError),
    };

    Ok(LocalPullFileResponse { result })
}
