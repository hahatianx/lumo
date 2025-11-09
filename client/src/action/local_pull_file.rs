use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::models::local_file::local_pull_file::LocalPullFileRequest;
use cli_handler::cli_impl;
use std::time::SystemTime;

#[cli_impl]
pub fn local_pull_file(
    src_file_path: &str,
    expected_checksum: Option<u64>,
) -> Result<(), ClientError> {
    let start_time = SystemTime::now();

    let conn = Connection::new(None)?;
    let res = conn.request(ApiRequestKind::LocalPullFile(LocalPullFileRequest {
        path: src_file_path.to_string(),
        expected_checksum,
    }))?;
    println!("{:?}", res);

    let end_time = SystemTime::now();
    println!(
        "Time elapsed: {:?}",
        end_time.duration_since(start_time).unwrap()
    );

    Ok(())
}
