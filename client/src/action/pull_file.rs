use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::models::file::pull_file::PullFileRequest;
use cli_handler::cli_impl;

type Checksum = u64;

#[cli_impl]
pub fn pull_file(
    peer_identifier: String,
    file_path: String,
    expected_checksum: Option<Checksum>,
) -> Result<(), ClientError> {
    let conn = Connection::new(None)?;

    let res = conn.request(ApiRequestKind::PullFile(PullFileRequest {
        peer_identifier,
        path: file_path.to_string(),
        expected_checksum,
    }))?;

    Ok(())
}
