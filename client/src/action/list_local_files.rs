use crate::action::conn::Connection;
use crate::error::ClientError;
use crate::extract_response;
use crate::format::table::{Schema, TableColumn, TableEntry, TableFormatter, format_table};
use crate::format::util::{system_time_to_human_readable, u64_to_human_readable};
use crate::format::xterm_color;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::message::api_response_message::ApiResponseKind;
use api_model::protocol::models::file::list_local_files::ListLocalFilesRequest;
use api_model::protocol::models::file::local_file::LocalFile;
use cli_handler::cli_impl;

static FULL_LOCAL_FILE_TABLE_SCHEMA: [&'static TableColumn; 8] = [
    &TableColumn {
        idx: 0,
        name: "Key",
    },
    &TableColumn {
        idx: 1,
        name: "Path",
    },
    &TableColumn {
        idx: 2,
        name: "Size",
    },
    &TableColumn {
        idx: 3,
        name: "Checksum",
    },
    &TableColumn {
        idx: 4,
        name: "Last author",
    },
    &TableColumn {
        idx: 5,
        name: "Last modified",
    },
    &TableColumn {
        idx: 6,
        name: "Active",
    },
    &TableColumn {
        idx: 7,
        name: "Freshness",
    },
];

pub struct FullLocalFileTable;

impl Schema<8> for FullLocalFileTable {
    fn names() -> [&'static TableColumn; 8] {
        FULL_LOCAL_FILE_TABLE_SCHEMA
    }
}

impl TableEntry<8, FullLocalFileTable> for LocalFile {
    fn fmt(&self) -> std::collections::HashMap<usize, String> {
        let mut map = std::collections::HashMap::new();
        map.insert(0, self.key.clone());
        map.insert(1, self.path.clone());
        map.insert(2, u64_to_human_readable(self.size));
        map.insert(
            3,
            match self.checksum {
                0 => "N/A".to_string(),
                _ => format!("{:016x}", self.checksum),
            },
        );
        map.insert(4, self.last_write.clone());
        map.insert(5, system_time_to_human_readable(self.last_modified));
        map.insert(
            6,
            match self.is_active {
                true => xterm_color::bold_green("Active"),
                false => xterm_color::bold_red("Inactive"),
            },
        );
        map.insert(
            7,
            match self.is_stale {
                true => xterm_color::bold_red("Stale"),
                false => xterm_color::bold_green("Fresh"),
            },
        );
        map
    }
}

#[cli_impl]
pub fn list_local_files() -> Result<(), ClientError> {
    let conn = Connection::new(None)?;

    let res = extract_response!(
        conn.request(ApiRequestKind::ListLocalFiles(ListLocalFilesRequest))?,
        ApiResponseKind::ListLocalFiles
    )?;

    let table_fmt = TableFormatter::<8, FullLocalFileTable>::new();
    let formatted_table = format_table(&table_fmt, &res.local_files);
    println!("{}", formatted_table);

    Ok(())
}
