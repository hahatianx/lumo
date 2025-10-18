//! A minimal async logger example using Tokio.
//!
//! This module shows how to build a very small, dependency-free (besides tokio)
//! asynchronous logger. It spawns a background task that receives log messages
//! over an mpsc channel and writes them to a file (or stdout) without blocking
//! your async tasks.
//!
//! Example
//! -------
//!
//! ```ignore
//! use server::utilities::logger::{init_file_logger, LogLevel};
//! use tokio::time::{sleep, Duration};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Start logger writing to a file (create if missing, append if exists)
//!     let (logger, _task) = init_file_logger("server.log").await.expect("init logger");
//!
//!     logger.log(LogLevel::Info, "Server starting up...");
//!     logger.info("Listening on 127.0.0.1:8080");
//!     logger.warn("This is a warning");
//!     logger.error("This is an error");
//!
//!     // Simulate some async work
//!     sleep(Duration::from_millis(50)).await;
//!
//!     // When `logger` is dropped, the channel closes and the background task
//!     // will flush and exit gracefully.
//! }
//! ```

use crate::err::Result;
use crate::global_var::{DEBUG_MODE, LOGGER_CELL};
use chrono::{DateTime, Utc};
use std::cmp::PartialEq;
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

/// Log level for messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LogLevel::Trace => "\x1b[36mTRACE\x1b[0m",
            LogLevel::Debug => "\x1b[34mDEBUG\x1b[0m",
            LogLevel::Info => "INFO ",
            LogLevel::Warn => "\x1b[33mWARN \x1b[0m",
            LogLevel::Error => "\x1b[31mERROR\x1b[0m",
        };
        write!(f, "{}", s)
    }
}

/// A simple async logger handle. Cloning creates another sender handle.
#[derive(Clone, Debug)]
pub struct AsyncLogger {
    tx: mpsc::Sender<LogRecord>,
}

impl AsyncLogger {
    /// Log a message at a specific level.
    fn log<S: Into<String>>(&self, level: LogLevel, msg: S) {
        let str_msg = msg.into();
        match self.tx.try_send(LogRecord::new(level, str_msg)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Failed to send log message: {}", err);
            }
        }
    }

    /// Request the logger task to flush and shut down.
    pub async fn shutdown(&self) {
        // Ignore send error (e.g., task already closed)
        let _ = self.tx.send(LogRecord::Shutdown).await;
    }

    pub fn trace<S: Into<String>>(&self, msg: S) {
        self.log(LogLevel::Trace, msg);
    }
    pub fn debug<S: Into<String>>(&self, msg: S) {
        if *DEBUG_MODE {
            self.log(LogLevel::Debug, msg);
        }
    }
    pub fn info<S: Into<String>>(&self, msg: S) {
        self.log(LogLevel::Info, msg);
    }
    pub fn warn<S: Into<String>>(&self, msg: S) {
        self.log(LogLevel::Warn, msg);
    }
    pub fn error<S: Into<String>>(&self, msg: S) {
        self.log(LogLevel::Error, msg);
    }
}

#[derive(Debug)]
enum LogRecord {
    Message {
        level: LogLevel,
        msg: String,
        ts_millis: i64,
    },
    Shutdown,
}

impl LogRecord {
    fn new(level: LogLevel, msg: String) -> Self {
        let ts_millis = Utc::now().timestamp_millis();
        Self::Message {
            level,
            msg,
            ts_millis,
        }
    }

    fn format_line(&self) -> Option<String> {
        match self {
            LogRecord::Message {
                level,
                msg,
                ts_millis,
            } => {
                // Format: 2025-10-08T21:22:33.123Z [LEVEL] message\n
                let dt = DateTime::from_timestamp_millis(*ts_millis).unwrap_or_else(|| Utc::now());
                let time_stamp = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                Some(format!("{} [{}] {}\n", time_stamp, level, msg))
            }
            LogRecord::Shutdown => None,
        }
    }
}

/// Initialize a file-based async logger. Returns the logger handle and the background task handle.
/// Dropping the last logger handle will close the channel and allow the task to shut down.
pub async fn init_file_logger<P: AsRef<Path>>(path: P) -> Result<(AsyncLogger, JoinHandle<()>)> {
    // Keep a copy of the path so we can reopen the file if a writing error occurs.
    let path_buf = path.as_ref().to_path_buf();

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path_buf)
        .await?;

    let (tx, mut rx) = mpsc::channel::<LogRecord>(1024);
    let writer = Mutex::new(BufWriter::new(file));

    let task = tokio::spawn(async move {
        while let Some(rec) = rx.recv().await {
            match &rec {
                LogRecord::Message {
                    level,
                    msg,
                    ts_millis,
                } => {
                    if let Some(line) = rec.format_line() {
                        {
                            let mut unique_writer = writer.lock().await;
                            let _ = unique_writer.write_all(line.as_bytes()).await;
                            let _ = unique_writer.flush().await;
                        }
                    }
                }
                LogRecord::Shutdown => {
                    break;
                }
            }
        }
        // Flush remaining data before exit
        {
            let mut unique_writer = writer.lock().await;
            let _ = unique_writer.flush().await;
        }
    });

    Ok((AsyncLogger { tx }, task))
}

pub(crate) struct Logger;

impl Deref for Logger {
    type Target = AsyncLogger;
    fn deref(&self) -> &Self::Target {
        if let Some(l) = LOGGER_CELL.get() {
            return l;
        }
        #[cfg(test)]
        {
            // In test builds, lazily install a fallback no-op logger so unit tests
            // can call LOGGER.*() without panicking even if the logger was not
            // explicitly initialized. The fallback keeps a channel alive but does
            // not spawn any async task or write to disk.
            let _ = LOGGER_CELL.set(test_fallback_logger());
            return LOGGER_CELL
                .get()
                .expect("LOGGER_CELL should be set by test fallback");
        }
        LOGGER_CELL.get().expect("LOGGER_CELL should be set")
    }
}

// A tiny, self-contained helper to avoid pulling chrono; build time is a best-effort.
#[cfg(test)]
fn test_fallback_logger() -> AsyncLogger {
    // Create a channel and leak the receiver to keep it alive without a runtime.
    let (tx, rx) = mpsc::channel::<LogRecord>(1024);
    let _ = Box::leak(Box::new(rx));
    AsyncLogger { tx }
}

#[cfg(test)]
mod tests {
    use super::LogRecord;
    use super::{LogLevel, init_file_logger};
    use chrono::{SecondsFormat, Utc};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut p = std::env::temp_dir();
        p.push(format!("{}_{}_{}.log", name, std::process::id(), millis));
        p
    }

    // RAII guard to ensure the temporary log file is removed on drop,
    // even if a test fails or panics before reaching explicit cleanup.
    struct TempFileGuard(PathBuf);
    impl TempFileGuard {
        fn new<P: AsRef<Path>>(path: P) -> Self {
            Self(path.as_ref().to_path_buf())
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[tokio::test]
    async fn test_file_logger_writes_lines() {
        let path = unique_temp_path("test_file_logger_writes_lines");
        let _guard = TempFileGuard::new(&path);
        let (logger, task) = init_file_logger(&path).await.expect("init logger");

        logger.info("hello info");
        logger.warn("be careful");
        logger.error("something went wrong");

        drop(logger); // close channel
        // Wait for background task to flush and exit
        task.await.expect("logger task join");

        let content = fs::read_to_string(&path).expect("read log file");

        assert!(
            content.contains("[INFO ] hello info"),
            "content=\n{}",
            content
        );
        assert!(
            content.contains("[\x1b[33mWARN \x1b[0m] be careful"),
            "content=\n{}",
            content
        );
        assert!(
            content.contains("[\x1b[31mERROR\x1b[0m] something went wrong"),
            "content=\n{}",
            content
        );
        assert!(
            content.ends_with('\n'),
            "log should end with newline; content=\n{}",
            content
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_log_level_display_strings() {
        assert_eq!(format!("{}", LogLevel::Trace), "\x1b[36mTRACE\x1b[0m");
        assert_eq!(format!("{}", LogLevel::Debug), "\x1b[34mDEBUG\x1b[0m");
        assert_eq!(format!("{}", LogLevel::Info), "INFO ");
        assert_eq!(format!("{}", LogLevel::Warn), "\x1b[33mWARN \x1b[0m");
        assert_eq!(format!("{}", LogLevel::Error), "\x1b[31mERROR\x1b[0m");
    }

    #[test]
    fn test_format_line_with_fixed_timestamp() {
        // Fixed at Unix epoch to make the output deterministic
        let rec = LogRecord::Message {
            level: LogLevel::Debug,
            msg: "xyz".into(),
            ts_millis: 0,
        };
        let line = rec.format_line().expect("line should exist for Message");
        println!("{}", &line);
        assert!(line.contains("[\x1b[34mDEBUG\x1b[0m]"));
        assert!(line.contains("xyz"));
        assert!(line.contains("1970-01-01"));
        assert!(line.contains('T'));
        assert!(line.contains('Z'));
        assert!(line.ends_with('\n'));

        // Also directly test the splitter
        let date_time =
            chrono::DateTime::<Utc>::from(UNIX_EPOCH).to_rfc3339_opts(SecondsFormat::Millis, true);
        assert!(date_time.starts_with("1970-01-01T00:00:00.000Z"));
    }

    #[tokio::test]
    async fn test_multiple_levels_format() {
        let path = unique_temp_path("test_multiple_levels_format");
        let _guard = TempFileGuard::new(&path);
        let (logger, task) = init_file_logger(&path).await.expect("init logger");

        logger.trace("trace msg");
        logger.info("info msg");
        logger.warn("warn msg");
        logger.error("error msg");

        drop(logger);
        task.await.expect("logger task join");

        let content = fs::read_to_string(&path).expect("read log file");

        // Each level marker should appear at least once
        for (marker, msg) in [
            ("[\x1b[36mTRACE\x1b[0m]", "trace msg"),
            ("[INFO ]", "info msg"),
            ("[\x1b[33mWARN \x1b[0m]", "warn msg"),
            ("[\x1b[31mERROR\x1b[0m]", "error msg"),
        ] {
            assert!(
                content.contains(marker),
                "missing level marker {} in\n{}",
                marker,
                content
            );
            assert!(
                content.contains(msg),
                "missing message '{}' in\n{}",
                msg,
                content
            );
        }

        // Basic shape check: RFC3339-ish timestamp with 'T' and trailing 'Z'
        // e.g., 2025-01-01T00:00:00.000Z [INFO] ...
        assert!(
            content.contains('T'),
            "timestamp should contain 'T':\n{}",
            content
        );
        assert!(
            content.contains('Z'),
            "timestamp should contain 'Z':\n{}",
            content
        );

        let _ = fs::remove_file(&path);
    }
}
