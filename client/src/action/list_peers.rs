use crate::action::conn::Connection;
use crate::error::ClientError;
use crate::extract_response;
use crate::format::table::{Schema, TableColumn, TableEntry, TableFormatter, format_table};
use crate::format::util;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use api_model::protocol::message::api_response_message::ApiResponseKind;
use api_model::protocol::models::peer::list_peers::{ListPeersRequest, Peer};
use cli_handler::cli_impl;

static FULL_PEER_TABLE_SCHEMA: [&'static TableColumn; 5] = [
    &TableColumn { idx: 0, name: "Id" },
    &TableColumn {
        idx: 1,
        name: "Name",
    },
    &TableColumn {
        idx: 0,
        name: "Addr",
    },
    &TableColumn {
        idx: 0,
        name: "Main",
    },
    &TableColumn {
        idx: 0,
        name: "Last seen",
    },
];

pub struct FullPeerTable;
impl Schema<5> for FullPeerTable {
    fn names() -> [&'static TableColumn; 5] {
        FULL_PEER_TABLE_SCHEMA
    }
}

impl TableEntry<5, FullPeerTable> for Peer {
    fn fmt(&self) -> std::collections::HashMap<usize, String> {
        let mut row = std::collections::HashMap::new();

        row.insert(0, self.identifier.clone());
        row.insert(1, self.peer_name.clone());
        row.insert(2, self.peer_addr.clone());
        row.insert(3, self.is_main.to_string());
        row.insert(4, util::readable_format(self.last_seen));
        row
    }
}

#[cli_impl]
pub fn list_peers() -> Result<(), ClientError> {
    let conn = Connection::new(None)?;

    let res = extract_response!(
        conn.request(ApiRequestKind::ListPeers(ListPeersRequest))?,
        ApiResponseKind::ListPeers
    )?;
    let table_fmt = TableFormatter::<5, FullPeerTable>::new();
    let formatted_table = format_table(&table_fmt, &res.peers);
    println!("{}", formatted_table);

    Ok(())
}
