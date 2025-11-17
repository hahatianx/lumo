use crate::interface::handlers::list_local_files::list_local_files;
use crate::interface::handlers::list_peers::list_peers;
use crate::interface::handlers::list_tasks::list_tasks;
use crate::interface::handlers::local_pull_file::local_pull_file;
use crate::interface::handlers::pull_file::pull_file;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::message::api_response_message::ApiResponseKind;

mod list_local_files;
pub mod list_peers;
pub mod list_tasks;
pub mod local_pull_file;
pub mod pull_file;

pub async fn run_handler(api_request_kind: &ApiRequestKind) -> crate::err::Result<ApiResponseKind> {
    let response = match api_request_kind {
        ApiRequestKind::ListPeers(req) => list_peers(req).await,
        ApiRequestKind::LocalPullFile(req) => local_pull_file(req).await,
        ApiRequestKind::ListTasks(req) => list_tasks(req).await,
        ApiRequestKind::PullFile(req) => pull_file(req).await,
        ApiRequestKind::ListLocalFiles(req) => list_local_files(req).await,
        _ => return Err(format!("Handler for {:?} not found", api_request_kind).into()),
    };
    Ok(response)
}
