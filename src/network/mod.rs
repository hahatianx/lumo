mod listener;
pub mod protocol;
mod sender;
pub use sender::NetworkSender;
mod util;

use crate::core::tasks::task_queue::TaskQueue;
use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::network::protocol::parse_message;
pub use util::get_private_ipv4_with_mac;

#[derive(Debug)]
pub struct NetworkSetup {
    pub sender: sender::NetworkSenderCore,

    // pub listener: listener::UdpListener,
    pub listener_handle: listener::ListenerHandle,
}

/// Initiate network connections and other setup tasks.
/// Setup UdpSender
/// Setup UdpListener
pub async fn init_network(task_queue: &TaskQueue) -> Result<NetworkSetup> {
    let udp_sender = sender::NetworkSenderCore::new_queue_worker(sender::SenderConfig::default());
    let udp_listener = listener::UdpListener::bind().await?;

    let task_queue_sender = task_queue.sender();
    let udp_join_handle = udp_listener.into_task(move |bytes, peer| {
        if peer.ip() == ENV_VAR.get().unwrap().get_ip_addr() {
            // Ignore packets from itself
            LOGGER.debug(format!("Ignoring packet from self: {:?}", peer));
            return;
        }
        match parse_message(&bytes) {
            Ok(msg) => {
                if let Err(e) = task_queue_sender.try_send(msg) {
                    LOGGER.error(format!("Unable to send message to task queue: {:?}", e));
                }
            }
            Err(e) => {
                LOGGER.error(format!(
                    "Unable to translate bytes into messages bytes: {:?}, peer: {:?}, error: {:?}",
                    bytes, peer, e
                ));
            }
        }
    });

    Ok(NetworkSetup {
        sender: udp_sender,
        listener_handle: udp_join_handle,
    })
}

pub async fn terminate_network(setup: NetworkSetup) -> Result<()> {
    let _ = setup.sender.shutdown().await;
    let _ = setup.listener_handle.shutdown().await?;
    Ok(())
}
