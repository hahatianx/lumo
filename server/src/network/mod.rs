mod udp_listener;
pub mod protocol;
mod udp_sender;
pub use udp_sender::NetworkSender;
mod util;

use crate::core::tasks::task_queue::TaskQueue;
use crate::err::Result;
use crate::global_var::LOGGER;
use crate::network::protocol::parse_message;
pub use util::get_private_ipv4_with_mac;

#[derive(Debug)]
pub struct NetworkSetup {
    pub sender: udp_sender::NetworkSenderCore,

    // pub listener: listener::UdpListener,
    pub listener_handle: udp_listener::ListenerHandle,
}

/// Initiate network connections and other setup tasks.
/// Setup UdpSender
/// Setup UdpListener
pub async fn init_network(task_queue: &TaskQueue) -> Result<NetworkSetup> {
    let udp_sender = udp_sender::NetworkSenderCore::new_queue_worker(udp_sender::SenderConfig::default());
    let udp_listener = udp_listener::UdpListener::bind().await?;

    let task_queue_sender = task_queue.sender();
    let udp_join_handle = udp_listener.into_task(move |bytes, peer| match parse_message(&bytes) {
        Ok(msg) => {
            if msg.should_ignore_by_sockaddr_peer(&peer) {
                return;
            }
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
