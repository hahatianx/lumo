use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::models::task::list_tasks::ListTasksRequest;
use cli_handler::cli_impl;

#[cli_impl]
pub fn list_tasks() -> Result<(), ClientError> {
    let conn = Connection::new(None)?;
    let res = conn.request(ApiRequestKind::ListTasks(ListTasksRequest))?;
    println!("{:?}", res);

    Ok(())
}
