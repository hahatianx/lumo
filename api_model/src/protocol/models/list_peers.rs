use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Peer {
    pub identifier: String,
    pub peer_name: String,
    pub peer_addr: String,

    pub is_main: bool,

    pub last_seen: SystemTime,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListPeersRequest;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListPeersResponse {
    pub peers: Vec<Peer>,
}
