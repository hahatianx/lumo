use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::models::task::list_tasks::ListTasksRequest;
use cli_handler::cli_impl;
use std::time::SystemTime;

#[cli_impl]
pub fn list_tasks() -> Result<(), ClientError> {
    let start_time = SystemTime::now();

    let conn = Connection::new(None)?;
    let res = conn.request(ApiRequestKind::ListTasks(ListTasksRequest))?;
    println!("{:?}", res);

    let end_time = SystemTime::now();

    println!(
        "Time elapsed: {:?}",
        end_time.duration_since(start_time).unwrap()
    );

    Ok(())
}
