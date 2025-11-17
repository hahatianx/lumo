use crate::core::tasks::{JOB_TABLE, JobSummary};
use crate::err::Result;
use api_model::protocol::models::task::list_tasks::{ListTasksRequest, ListTasksResponse};
use api_model::protocol::models::task::task::Task;
use cli_handler::cli_handler;

#[cli_handler(ListTasks)]
pub async fn list_tasks(_request: &ListTasksRequest) -> Result<ListTasksResponse> {
    let job_summary_table = JOB_TABLE.fetch_job_details().await?;

    let tasks = job_summary_table
        .into_iter()
        .filter(|j| j.is_some())
        .map(move |job| {
            let JobSummary {
                job_id,
                job_name,
                launched_time,
                complete_time,
                status,
                status_msg,
                job_type,
                period,
                summary,
                shutdown_tx: _,
            } = job.unwrap();
            Task {
                job_id,
                job_name,
                summary,
                launch_time: launched_time.into(),
                complete_time: complete_time.map(|t| t.into()),
                status: status.into(),
                status_message: status_msg,
                job_type: job_type.into(),
                period: period.map(|p| p.num_seconds() as u64),
            }
        })
        .collect();

    Ok(ListTasksResponse { tasks })
}
