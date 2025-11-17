use crate::action::conn::Connection;
use crate::error::ClientError;
use crate::extract_response;
use crate::format::table::{Schema, TableColumn, TableEntry, TableFormatter, format_table};
use crate::format::util::str_safe_truncate;
use crate::format::{util, xterm_color};
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::message::api_response_message::ApiResponseKind;
use api_model::protocol::models::task::list_tasks::ListTasksRequest;
use api_model::protocol::models::task::task::{JobStatus, JobType, Task};
use cli_handler::cli_impl;
use std::collections::HashMap;

static FULL_TASK_TABLE_SCHEMA: [&'static TableColumn; 10] = [
    &TableColumn { idx: 0, name: "Id" },
    &TableColumn {
        idx: 0,
        name: "Name",
    },
    &TableColumn {
        idx: 0,
        name: "Summary",
    },
    &TableColumn {
        idx: 0,
        name: "Launch time",
    },
    &TableColumn {
        idx: 0,
        name: "Complete time",
    },
    &TableColumn {
        idx: 0,
        name: "Status",
    },
    &TableColumn {
        idx: 0,
        name: "Status message",
    },
    &TableColumn {
        idx: 0,
        name: "Job type",
    },
    &TableColumn {
        idx: 0,
        name: "Runs every (sec)",
    },
    &TableColumn {
        idx: 0,
        name: "Times out after (sec)",
    },
];

pub struct FullTaskTable;
impl Schema<10> for FullTaskTable {
    fn names() -> [&'static TableColumn; 10] {
        FULL_TASK_TABLE_SCHEMA
    }
}

pub struct JobStatusDisplay(pub JobStatus);
impl JobStatusDisplay {
    fn display(&self) -> String {
        match self.0 {
            JobStatus::Running => String::from("Running"),
            JobStatus::Completed => xterm_color::bold_green("Completed"),
            JobStatus::Failed => xterm_color::bold_red("Failed"),
            JobStatus::TimedOut => xterm_color::bold_yellow("Timed out"),
            JobStatus::Pending => xterm_color::bold("Pending"),
            JobStatus::Shutdown => xterm_color::bold("Shutdown"),
            _ => xterm_color::red("Unknown"),
        }
    }
}

impl TableEntry<10, FullTaskTable> for Task {
    fn fmt(&self) -> HashMap<usize, String> {
        let mut row = std::collections::HashMap::new();

        row.insert(0, format!("{:016x}", self.job_id));
        row.insert(1, util::str_safe_truncate(&self.job_name, 20));
        row.insert(2, str_safe_truncate(&self.summary, 50));
        row.insert(3, util::system_time_to_human_readable(self.launch_time));
        row.insert(
            4,
            match self.complete_time {
                Some(time) => util::system_time_to_human_readable(time),
                None => String::from("-"),
            },
        );
        row.insert(5, JobStatusDisplay(self.status).display());
        row.insert(
            6,
            self.status_message
                .as_ref()
                .map_or_else(|| String::from(""), |msg| str_safe_truncate(msg, 150)),
        );
        match self.job_type {
            JobType::Periodic => {
                row.insert(7, String::from("Periodic"));
                row.insert(
                    8,
                    self.period
                        .map_or_else(|| String::from("-"), |period| period.to_string()),
                );
                row.insert(9, String::from("-"));
            }
            JobType::OneTime => {
                row.insert(7, String::from("OneTime"));
                row.insert(8, String::from("-"));
                row.insert(
                    9,
                    self.period
                        .map_or_else(|| String::from("-"), |period| period.to_string()),
                );
            }
            JobType::Claimable => {
                row.insert(7, String::from("Claimable"));
                row.insert(8, String::from("-"));
                row.insert(
                    9,
                    self.period
                        .map_or_else(|| String::from("-"), |period| period.to_string()),
                );
            }
        }

        row
    }
}

#[cli_impl]
pub fn list_tasks() -> Result<(), ClientError> {
    let conn = Connection::new(None)?;

    let res = extract_response!(
        conn.request(ApiRequestKind::ListTasks(ListTasksRequest))?,
        ApiResponseKind::ListTasks
    )?;
    let table_fmt = TableFormatter::<10, FullTaskTable>::new();
    let formatted_table = format_table(&table_fmt, &res.tasks);
    println!("{}", formatted_table);

    Ok(())
}
