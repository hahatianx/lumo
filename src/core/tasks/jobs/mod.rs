mod job_peer_table_anti_entropy;

use crate::err::Result;
pub use job_fs_anti_entropy::{job_fs_inactive_cleanup, job_fs_stale_rescan};
pub use job_heartbeat::{get_first_hello_message_closure, get_job_heartbeat_closure};
pub use job_peer_table_anti_entropy::job_peer_table_anti_entropy;
use std::future::Future;
use std::pin::Pin;
mod job_fs_anti_entropy;
mod job_heartbeat;
mod oneshot_job;
mod periodic_job;

use crate::core::tasks::job_summary::JobStatus;
pub use oneshot_job::launch_oneshot_job;
pub use periodic_job::launch_periodic_job;

// A boxed closure that yields a boxed, pinned Future resolving to Result<()>.
pub type JobClosure =
    dyn FnMut() -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> + Send + 'static;
pub type CallbackFunction = dyn FnMut(JobStatus, String) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>
    + Send
    + 'static;
