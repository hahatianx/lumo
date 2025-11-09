use crate::core::PEER_TABLE;
use crate::err::Result;
use api_model::protocol::models::peer::list_peers::{ListPeersRequest, ListPeersResponse, Peer};
use cli_handler::cli_handler;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ms_to_system_time(last_seen_ms: u64, time_zone: i32) -> SystemTime {
    UNIX_EPOCH
        + Duration::from_millis(last_seen_ms)
        + Duration::from_millis(time_zone as u64 * 60 * 1000)
}

#[cli_handler(ListPeers)]
pub async fn list_peers(_request: &ListPeersRequest) -> Result<ListPeersResponse> {
    let peers = PEER_TABLE.get_peers().await;

    let peer_response: Vec<Peer> = peers
        .iter()
        .filter(|p| p.is_active.load(Ordering::Relaxed))
        .map(|p| Peer {
            identifier: p.identifier.clone(),
            peer_name: p.peer_name.clone(),
            peer_addr: p.peer_addr.to_string(),
            is_main: p.is_main.load(Ordering::Relaxed),
            last_seen: ms_to_system_time(
                p.last_seen_ms.load(Ordering::Relaxed),
                p.last_seen_tz_offset_minutes.load(Ordering::Relaxed),
            ),
        })
        .collect();

    Ok(ListPeersResponse {
        peers: peer_response,
    })
}
