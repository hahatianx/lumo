mod util;

use crate::err::Result;
use crate::utilities::init_file_logger;
use std::path::{Path, PathBuf};
use std::fs;
use tokio::task::JoinHandle;
use crate::utilities::AsyncLogger;

/// Initialize filesystem-related resources under the given `path`.
///
/// Steps:
/// 1. Verify the directory exists and we have read, write, and execute permissions.
/// 2. Get or create a ".disc" subdirectory.
/// 3. Get or create a "logs" subdirectory under ".disc".
/// 4. Initialize the async file logger with a log file in the logs directory.
///
/// Returns the async logger handle and the background task handle.
pub async fn init_fs<P: AsRef<Path>>(path: P) -> Result<(AsyncLogger, JoinHandle<()>)> {
    let base: &Path = path.as_ref();

    // 1. Check permissions on the provided path.
    let perms = util::check_dir_permissions(base);
    if !(perms.read && perms.write && perms.execute) {
        return Err(format!(
            "Insufficient permissions for path '{}': read={}, write={}, execute={}",
            base.display(), perms.read, perms.write, perms.execute
        ).into());
    }

    // 2. Get or create .disc directory.
    let disc_dir: PathBuf = base.join(".disc");
    fs::create_dir_all(&disc_dir)?;

    // 3. Get or create logs directory under .disc.
    let logs_dir: PathBuf = disc_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;

    // 4. Initialize async file logger directing output to logs directory.
    let log_file: PathBuf = logs_dir.join("server.log");
    let (logger, task) = init_file_logger(&log_file).await?;

    Ok((logger, task))
}