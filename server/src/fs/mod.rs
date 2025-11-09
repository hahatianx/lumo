pub mod file;
mod fs_listener;
pub mod util;
pub use file::LumoFile;
mod fs_index;
pub use fs_index::FS_INDEX;
pub use fs_index::init_fs_index;
mod fs_lock;
mod fs_op;
mod task_management;
pub use task_management::file_request_tasks::{
    PendingPull, PullRequestResult, RejectionReason, cancel_pending, claim_by_nonce,
    start_pull_request,
};

pub use fs_listener::FsListener;

use crate::err::Result;
use crate::fs::util::test_dir_existence;
use crate::utilities::AsyncLogger;
use crate::utilities::init_file_logger;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

/// Initialize filesystem-related resources under the given `path`.
///
/// Steps:
/// 1. Verify the directory exists, and we have read, write, and execute permissions.
/// 2. Get or create a ".disc" subdirectory.
/// 3. Get or create a "logs" subdirectory under ".disc".
/// 4. Initialize the async file logger with a log file in the logs directory.
///
/// Returns the async logger handle and the background task handle.
pub async fn init_working_dir<P: AsRef<Path>>(path: P) -> Result<(AsyncLogger, JoinHandle<()>)> {
    let base: &Path = path.as_ref();

    // 1. Check permissions on the provided path.
    if !test_dir_existence(base) {
        return Err(format!("Directory '{}' does not exist", base.display()).into());
    }
    let perms = util::check_dir_permissions(base);
    if !(perms.read && perms.write && perms.execute) {
        return Err(format!(
            "Insufficient permissions for path '{}': read={}, write={}, execute={}",
            base.display(),
            perms.read,
            perms.write,
            perms.execute
        )
        .into());
    }

    // 2. Get or create a.disc directory.
    let disc_dir: PathBuf = base.join(".disc");
    fs::create_dir_all(&disc_dir)?;

    // 3. Get or create a logs directory under .disc.
    let logs_dir: PathBuf = disc_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;

    // 4. Get or create a tmp directory under .disc., this folder will hold temporary downloads
    let tmp_download_dir: PathBuf = disc_dir.join("tmp_downloads");
    fs::create_dir_all(&tmp_download_dir)?;

    // 5. Initialize the async file logger directing output to the logs' directory.
    let log_file: PathBuf = logs_dir.join("server.log");
    let (logger, task) = init_file_logger(&log_file).await?;

    Ok((logger, task))
}

pub async fn init_fs<P: AsRef<Path>>(base: P) -> Result<()> {
    // Start filesystem watcher and spawn a background processor for events
    let (listener, rx) = FsListener::watch(&base).expect("should start watcher");

    // Keep the watcher alive for the lifetime of the process
    let _leaked_watcher: &'static mut FsListener = Box::leak(Box::new(listener));

    // Spawn a task to process all notify events according to the required algorithm
    let _processor = FsListener::spawn_default_processor(rx);

    init_fs_index().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use std::time::Duration;

    // RAII guard to ensure the temporary directory tree is deleted on drop,
    // even if the test fails/panics early.
    struct TempDirGuard(std::path::PathBuf);
    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let mut p = std::env::temp_dir();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            p.push(format!("{}_{}_{}", prefix, std::process::id(), ts));
            fs::create_dir_all(&p).unwrap();
            TempDirGuard(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn init_fs_creates_disc_logs_and_logfile_and_writes() {
        let tmp = TempDirGuard::new("init_fs_ok");
        let temp_dir = tmp.path();

        let (logger, task) = init_working_dir(temp_dir)
            .await
            .expect("init_fs should succeed");

        // Emit a couple of log lines
        logger.info("hello world");
        logger.error("boom");
        // Drop the logger to close the channel so the task can finish
        drop(logger);

        // Join the background task with a timeout to avoid hanging tests
        let join_res = tokio::time::timeout(Duration::from_secs(2), task).await;
        assert!(join_res.is_ok(), "logger task did not finish in time");
        assert!(join_res.unwrap().is_ok(), "logger task join error");

        // Check directories and file
        let disc = temp_dir.join(".disc");
        let logs = disc.join("logs");
        let logfile = logs.join("server.log");
        assert!(disc.is_dir(), ".disc directory should exist");
        assert!(logs.is_dir(), "logs directory should exist");
        assert!(logfile.is_file(), "server.log should exist");

        // Verify file contains our messages
        let mut content = String::new();
        let mut f = fs::File::open(&logfile).unwrap();
        f.read_to_string(&mut content).unwrap();
        assert!(content.contains("hello world"));
        assert!(content.contains("boom"));

        // Best-effort explicit cleanup (handled by Drop as well)
        let _ = fs::remove_file(logfile);
    }

    #[tokio::test]
    async fn init_fs_errors_when_path_is_file() {
        let tmp = TempDirGuard::new("init_fs_err");
        let temp_dir = tmp.path();
        let file_path = temp_dir.join("not_a_dir.txt");
        fs::write(&file_path, b"x").unwrap();

        let res = init_working_dir(&file_path).await;
        assert!(
            res.is_err(),
            "init_fs should error when given a non-directory path"
        );

        // Best-effort explicit cleanup (Drop will remove the directory tree)
        let _ = fs::remove_file(&file_path);
    }
}
