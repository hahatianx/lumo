mod listener;
mod protocol;
mod sender;
mod util;

use crate::err::Result;
pub use util::get_private_ipv4_with_mac;

/// Initiate network connections and other setup tasks.
/// Setup UdpSender
/// Setup UdpListener
pub fn init_network() -> Result<()> {
    let sender = sender::NetworkSender::new_queue_worker(sender::SenderConfig::default());

    Ok(())
}
