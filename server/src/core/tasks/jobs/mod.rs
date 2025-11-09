mod job_peer_table_anti_entropy;

use crate::err::Result;
pub use job_fs_anti_entropy::{job_fs_inactive_cleanup, job_fs_stale_rescan};
pub use job_fs_index_dump::get_job_fs_index_dump_closure;
pub use job_heartbeat::{get_first_hello_message_closure, get_job_heartbeat_closure};
pub use job_peer_table_anti_entropy::job_peer_table_anti_entropy;
use std::future::Future;
use std::pin::Pin;
mod job_fs_anti_entropy;
mod job_fs_index_dump;
pub mod job_genre;
mod job_heartbeat;

// Re-export claimable job utilities for external modules
pub use job_genre::claimable_job::{ClaimableJobHandle, launch_claimable_job};

use crate::core::tasks::job_summary::JobStatus;
pub use job_genre::oneshot_job::launch_oneshot_job;
pub use job_genre::periodic_job::launch_periodic_job;

// A boxed closure that yields a boxed, pinned Future resolving to Result<()>.
pub type JobClosure =
    dyn FnMut() -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> + Send + 'static;
pub type JobSummaryStatusCallback = dyn FnMut(JobStatus, String) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>
    + Send
    + 'static;
