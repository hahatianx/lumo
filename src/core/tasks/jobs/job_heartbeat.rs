use crate::core::PEER_TABLE;
use crate::core::tasks::jobs::periodic_job::JobClosure;
use crate::core::tasks::low_level_tasks::{SendControlMessageTask, SendType};
use crate::core::tasks::task_queue::TaskQueue;
use crate::err::Result;
use crate::network::protocol::messages::HelloMessage;
use crate::network::protocol::protocol::Protocol;
use bytes::Bytes;
use std::future::Future;
use std::net::IpAddr::{V4, V6};
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use crate::constants::UPD_MESSAGE_PORT;
use crate::global_var::LOGGER;

pub async fn get_job_heartbeat_closure(task_q: &TaskQueue) -> Result<Box<JobClosure>> {
    let task_q_sender = task_q.sender();

    // At the beginning of the job, send a broadcast HelloMessage to all nodes in the network.  The message requires response.
    let first_hello_message = HelloMessage::from_env(1)?;
    let b = Bytes::from(first_hello_message.serialize());
    task_q_sender.send(Box::new(SendControlMessageTask::new(SendType::Broadcast, b.clone()))).await?;

    // Return a closure compatible with launch_periodic_job: FnMut() -> Future<Output = Result<()>>
    let closure = move || {
        let cloned_task_q_sender = task_q_sender.clone();
        let fut: std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> =
            Box::pin(async move {

                let active_peers = PEER_TABLE
                    .get_peers()
                    .await
                    .iter()
                    .filter(|p| p.is_active.load(std::sync::atomic::Ordering::Relaxed))
                    .cloned()
                    .collect::<Vec<_>>();

                LOGGER.debug(format!("Active peers: {:?}", active_peers));

                let hello_message = HelloMessage::from_env(0)?;
                let bytes = Bytes::from(hello_message.serialize());

                for peer in active_peers {
                    let peer_sock = match peer.peer_addr {
                        V4(ipv4) => SocketAddr::V4(SocketAddrV4::new(ipv4, UPD_MESSAGE_PORT)),
                        V6(ipv6) => SocketAddr::V6(SocketAddrV6::new(ipv6, UPD_MESSAGE_PORT, 0, 0)),
                    };

                    LOGGER.debug(format!("Sending HelloMessage to peer {}, message {:?}", peer_sock, &hello_message));

                    let task =
                        SendControlMessageTask::new(SendType::Unicast(peer_sock), bytes.clone());

                    cloned_task_q_sender.send(Box::new(task)).await?;
                }

                Ok(())
            });
        fut
    };

    Ok(Box::new(closure))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::EnvVar;
    use crate::core::tasks::task_queue::TaskQueueConfig;
    use crate::global_var::ENV_VAR;

    // When there are no active peers, the heartbeat closure should still build and run without errors.
    // This also verifies HelloMessage::from_env() path works when ENV_VAR is initialized.
    #[tokio::test]
    async fn heartbeat_closure_runs_with_no_active_peers() -> Result<()> {
        // Initialize ENV_VAR once for the process (ignore if already set by other tests).
        if ENV_VAR.get().is_none() {
            let mut cfg = Config::new();
            cfg.identity.machine_name = "test-machine".into();
            cfg.identity.private_key_loc = "~/.ssh/id_rsa".into();
            cfg.identity.public_key_loc = "~/.ssh/id_rsa.pub".into();
            cfg.connection.conn_token = "TOKEN".into();
            cfg.app_config.working_dir = "~/tmp".into();
            let ev = EnvVar::from_config(&cfg)?;
            let _ = ENV_VAR.set(ev);
        }

        // Create a task queue; there are no peers so no sends will be enqueued.
        let q = TaskQueue::new(TaskQueueConfig { queue_bound: 8 });

        // Build the heartbeat job closure and run it once.
        let mut closure = get_job_heartbeat_closure(&q).await?;
        (closure)().await?;

        // Shutdown the queue to clean up background task.
        q.shutdown().await?;
        Ok(())
    }
}
