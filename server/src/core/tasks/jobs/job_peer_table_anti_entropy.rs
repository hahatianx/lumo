//! Task: Peer table anti-entropy job.
//!
//! This module exposes a tiny async job function that triggers the peer table
//! "anti-entropy" routine. Anti-entropy here means periodically scanning the
//! known peers and disabling those that have expired or become invalid, helping
//! the node converge to a consistent and healthy view of the cluster.
//!
//! Typical usage is to schedule this function to run periodically via the
//! `PeriodicJob` helper and the task queue infrastructure.
//!
//! See also:
//! - [`crate::core::topology::peer_table::PeerTable::peer_table_anti_entropy`]
//!   for the underlying logic.
//! - [`crate::core::tasks::jobs::periodic_job::launch_periodic_job`] for how to
//!   schedule periodic jobs.

use crate::core::PEER_TABLE;
use crate::err::Result;

/// Runs the peer-table anti-entropy routine once.
///
/// This is a thin wrapper that forwards to
/// [`PeerTable::peer_table_anti_entropy`](crate::core::topology::peer_table::PeerTable::peer_table_anti_entropy)
/// on the global `PEER_TABLE` instance.
///
/// Returns
/// - `Ok(())` if the scan completes successfully.
/// - `Err` if the underlying peer-table operation fails.
///
/// Example
/// ```ignore
/// use server::core::tasks::jobs::periodic_job::launch_periodic_job;
/// use server::core::tasks::task_queue::{TaskQueue, TaskQueueConfig};
/// use server::core::tasks::jobs::job_peer_table_anti_entropy::job_peer_table_anti_entropy;
/// # async fn demo() -> server::err::Result<()> {
/// let queue = TaskQueue::new(TaskQueueConfig { queue_bound: 128 });
/// let sender = queue.sender();
/// let summary = launch_periodic_job(
///     "peer-table-anti-entropy",
///     "Disables expired/invalid peers to keep the view fresh",
///     || job_peer_table_anti_entropy(),
///     5, // run every 5 seconds
///     sender,
/// ).await?;
/// // Later, when shutting down:
/// summary.shutdown().await?;
/// # Ok(())
/// # }
/// ```
pub async fn job_peer_table_anti_entropy() -> Result<()> {
    PEER_TABLE.peer_table_anti_entropy().await
}
