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
//! ```no_run
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
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Log level for messages.
#[derive(Clone, Copy, Debug)]
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
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
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
        if *DEBUG_MODE {
            println!("{}: {}", level, &str_msg);
        }
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
        ts_millis: i128,
    },
    Shutdown,
}

impl LogRecord {
    fn new(level: LogLevel, msg: String) -> Self {
        let ts_millis = chrono_like::now_millis();
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
                let (date, time_millis) = chrono_like::split_iso8601(*ts_millis);
                Some(format!(
                    "{}Z [{}] {}\n",
                    format!("{}T{}", date, time_millis),
                    level,
                    msg
                ))
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
    let mut writer = BufWriter::new(file);

    let task = tokio::spawn(async move {
        while let Some(rec) = rx.recv().await {
            match rec {
                LogRecord::Message { .. } => {
                    if let Some(line) = rec.format_line() {
                        if let Err(_e) = writer.write_all(line.as_bytes()).await {
                            // Attempt to recover: flush, reopen the file, swap the writer, and retry once.
                            let _ = writer.flush().await;
                            match OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&path_buf)
                                .await
                            {
                                Ok(new_file) => {
                                    writer = BufWriter::new(new_file);
                                    // Best-effort: try to write the original line again; if it fails, drop it.
                                    let _ = writer.write_all(line.as_bytes()).await;
                                }
                                Err(_) => {
                                    // Couldn't reopen. Drop the message and avoid tight loop.
                                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                                }
                            }
                        }
                    }
                }
                LogRecord::Shutdown => {
                    break;
                }
            }
        }
        // Flush remaining data before exit
        let _ = writer.flush().await;
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

mod chrono_like {
    // We avoid bringing chrono as a runtime dependency by using std::time.
    // This provides millisecond precision and a minimal ISO8601 formatter.
    pub fn now_millis() -> i128 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        now.as_millis() as i128
    }

    pub fn split_iso8601(ts_millis: i128) -> (String, String) {
        // Convert millis to seconds and remainder
        let secs = (ts_millis / 1000) as i64;
        let millis = (ts_millis % 1000) as i64;

        // Convert seconds to UTC date/time using chrono-like formatting via time crate logic.
        // But since we cannot import additional crates here, we do a minimal RFC3339-ish string
        // by using chrono math assumptions; this is approximate and sufficient for logging.
        // We will format as YYYY-MM-DD and HH:MM:SS.mmm using UTC from std::time.
        use std::time::{Duration, UNIX_EPOCH};
        let dt = UNIX_EPOCH + Duration::from_secs(secs as u64);
        let datetime: time_parts::Parts = time_parts::from_system_time(dt);
        let date = format!(
            "{:04}-{:02}-{:02}",
            datetime.year, datetime.month, datetime.day
        );
        let time = format!(
            "{:02}:{:02}:{:02}.{:03}",
            datetime.hour,
            datetime.minute,
            datetime.second,
            millis.abs()
        );
        (date, time)
    }

    mod time_parts {
        use std::time::SystemTime;

        // Very small date-time conversion (UTC) without external crates.
        // Not leap-second aware; good enough for logging.
        #[derive(Clone, Copy)]
        pub struct Parts {
            pub year: i32,
            pub month: u32,
            pub day: u32,
            pub hour: u32,
            pub minute: u32,
            pub second: u32,
        }

        pub fn from_system_time(st: SystemTime) -> Parts {
            // Use libc time functions via time_t conversion when available; otherwise, fallback.
            // For portability in this simple example, we use the chrono-less algorithm based on days since epoch.
            use std::time::{Duration, UNIX_EPOCH};
            let dur = st
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0));
            let mut secs = dur.as_secs() as i64;

            let second = (secs % 60) as u32;
            secs /= 60;
            let minute = (secs % 60) as u32;
            secs /= 60;
            let hour = (secs % 24) as u32;
            secs /= 24;

            // Days since 1970-01-01
            let days = secs as i64;
            let (year, month, day) = days_to_ymd(days + 719468); // shift to Civil (0000-03-01 base)

            Parts {
                year,
                month,
                day,
                hour,
                minute,
                second,
            }
        }

        // Algorithm adapted from Howard Hinnant's date algorithms (public domain)
        fn days_to_ymd(mut z: i64) -> (i32, u32, u32) {
            let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
            let doe = z - era * 146097; // [0, 146096]
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
            let y = yoe as i32 + era as i32 * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
            let mp = (5 * doy + 2) / 153; // [0, 11]
            let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
            let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]
            let year = y + (m <= 2) as i32;
            let month = m as u32;
            let day = d as u32;
            (year, month, day)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LogLevel, init_file_logger};
    use super::{LogRecord, chrono_like};
    use std::fs;
    use std::path::PathBuf;
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

    #[tokio::test]
    async fn test_file_logger_writes_lines() {
        let path = unique_temp_path("test_file_logger_writes_lines");
        let (logger, task) = init_file_logger(&path).await.expect("init logger");

        logger.info("hello info");
        logger.warn("be careful");
        logger.error("something went wrong");

        drop(logger); // close channel
        // Wait for background task to flush and exit
        task.await.expect("logger task join");

        let content = fs::read_to_string(&path).expect("read log file");

        assert!(
            content.contains("[INFO] hello info"),
            "content=\n{}",
            content
        );
        assert!(
            content.contains("[WARN] be careful"),
            "content=\n{}",
            content
        );
        assert!(
            content.contains("[ERROR] something went wrong"),
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
        assert_eq!(format!("{}", LogLevel::Trace), "TRACE");
        assert_eq!(format!("{}", LogLevel::Debug), "DEBUG");
        assert_eq!(format!("{}", LogLevel::Info), "INFO");
        assert_eq!(format!("{}", LogLevel::Warn), "WARN");
        assert_eq!(format!("{}", LogLevel::Error), "ERROR");
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
        assert!(line.contains("[DEBUG]"));
        assert!(line.contains("xyz"));
        assert!(line.contains("1970-01-01"));
        assert!(line.contains('T'));
        assert!(line.contains('Z'));
        assert!(line.ends_with('\n'));

        // Also directly test the splitter
        let (d, t) = chrono_like::split_iso8601(0);
        assert_eq!(d, "1970-01-01");
        assert!(t.starts_with("00:00:00."));
    }

    #[tokio::test]
    async fn test_multiple_levels_format() {
        let path = unique_temp_path("test_multiple_levels_format");
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
            ("[TRACE]", "trace msg"),
            ("[INFO]", "info msg"),
            ("[WARN]", "warn msg"),
            ("[ERROR]", "error msg"),
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
