use crate::err::Result;
use std::sync::LazyLock;

mod peer_table;
pub use peer_table::{Peer, PeerTable};

pub static PEER_TABLE: LazyLock<PeerTable> = LazyLock::new(|| PeerTable::new());

pub fn init_topology() -> Result<&'static PeerTable> {
    Ok(&PEER_TABLE)
}
