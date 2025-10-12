use crate::core::topology::{PEER_TABLE, Peer};
use crate::err::Result;
use crate::network::protocol::messages::HelloMessage;
use std::net::IpAddr;
use std::str::FromStr;

fn generate_peer_from_hello_message(msg: &HelloMessage) -> Result<Peer> {
    let ip_addr = IpAddr::from_str(msg.from_ip.as_str())?;
    Ok(Peer::new(
        msg.mac_addr.clone(),
        msg.from_name.clone(),
        ip_addr,
        false,
    ))
}

pub async fn update_peer_table(msg: &HelloMessage) -> Result<()> {
    let peer = generate_peer_from_hello_message(msg)?;
    PEER_TABLE.update_peer(peer).await
}

fn refresh_peer() -> Result<()> {
    Ok(())
}
