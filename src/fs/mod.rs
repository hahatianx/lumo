pub mod util;

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
pub async fn init_fs<P: AsRef<Path>>(path: P) -> Result<(AsyncLogger, JoinHandle<()>)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use std::time::Duration;

    // Use a helper to make a unique temp directory path
    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        p.push(format!("{}_{}_{}", name, std::process::id(), ts));
        p
    }

    #[tokio::test]
    async fn init_fs_creates_disc_logs_and_logfile_and_writes() {
        let temp_dir = unique_temp_dir("init_fs_ok");
        fs::create_dir_all(&temp_dir).unwrap();

        let (logger, task) = init_fs(&temp_dir).await.expect("init_fs should succeed");

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

        // Cleanup best-effort
        let _ = fs::remove_file(logfile);
        let _ = fs::remove_dir(logs);
        let _ = fs::remove_dir(disc);
        let _ = fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn init_fs_errors_when_path_is_file() {
        let temp_dir = unique_temp_dir("init_fs_err");
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("not_a_dir.txt");
        fs::write(&file_path, b"x").unwrap();

        let res = init_fs(&file_path).await;
        assert!(
            res.is_err(),
            "init_fs should error when given a non-directory path"
        );

        // Cleanup
        let _ = fs::remove_file(&file_path);
        let _ = fs::remove_dir(&temp_dir);
    }
}
