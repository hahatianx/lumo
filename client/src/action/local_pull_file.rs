use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::models::local_file::local_pull_file::LocalPullFileRequest;
use cli_handler::cli_impl;

#[cli_impl]
pub fn local_pull_file(
    src_file_path: &str,
    expected_checksum: Option<u64>,
) -> Result<(), ClientError> {
    let conn = Connection::new(None)?;
    let res = conn.request(ApiRequestKind::LocalPullFile(LocalPullFileRequest {
        path: src_file_path.to_string(),
        expected_checksum,
    }))?;
    println!("{:?}", res);

    Ok(())
}
