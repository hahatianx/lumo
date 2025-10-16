use crate::global_var::LOGGER;
use notify::event::{EventKind, ModifyKind};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver};

/// A simple filesystem listener built on top of `notify`.
///
/// Usage:
/// let (listener, rx) = FsListener::watch(path, true)?; // keep `listener` alive while using `rx`
pub struct FsListener {
    // Keep the watcher around so the OS resources and callback remain active.
    _watcher: RecommendedWatcher,
    // Store the watched root primarily for debugging/inspection.
    _root: PathBuf,
}

impl FsListener {
    /// Start watching the given path. If `recursive` is true, all subdirectories will be watched.
    /// Returns a tuple of (FsListener, Receiver<Event>). Keep the FsListener alive while receiving events.
    pub fn watch<P: AsRef<Path>>(path: P) -> crate::err::Result<(Self, Receiver<Event>)> {
        let root = path.as_ref().to_path_buf();
        if !root.exists() {
            return Err(format!("Path '{}' does not exist", root.display()).into());
        }

        // Channel for delivering events to async contexts
        let (tx, rx) = mpsc::channel(128);

        // Clone for the closure
        let tx_cloned = tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                LOGGER.debug(format!("Filesystem event: {:?}", &res));
                if let Ok(ev) = res {
                    if let Some(ev) = filter_event(ev) {
                        // Best-effort send it; ignore if receiver dropped
                        println!("{:?}", ev);
                        let _ = tx_cloned.blocking_send(ev);
                    } else {
                        // Filtered out; nothing to do
                        return;
                    }
                } else {
                    // on errors
                    println!("{:?}", &res);
                    // LOGGER.error(format!("Filesystem watcher error: {}", res.unwrap_err()));
                }
            },
            Config::default()
                .with_poll_interval(Duration::from_secs(5))
                .with_follow_symlinks(false),
        )?;

        // Begin watching
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let metadata_folder = root.join(".disc");
        if metadata_folder.exists() {
            // Best-effort: if .disc exists and was included by recursive watch, attempt to exclude it.
            // Ignore errors in case it wasn't explicitly watched.
            let _ = watcher.unwatch(&metadata_folder);
        }

        println!("Start watching {:?}", &watcher);

        Ok((
            Self {
                _watcher: watcher,
                _root: root,
            },
            rx,
        ))
    }

    /// Spawn a background task that processes all notify events according to the
    /// algorithm: if path exists => move into scope; otherwise => move out of scope.
    /// Returns a JoinHandle for the spawned task.
    pub fn spawn_default_processor(mut rx: Receiver<Event>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                for p in ev.paths {
                    let exists = p.exists();
                    if exists {
                        // Treat as move into watch scope
                        println!("[fs] move into scope: {}", p.display());
                    } else {
                        // Treat as move out of watch scope
                        println!("[fs] move out of scope: {}", p.display());
                    }
                }
            }
        })
    }
}

fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Common OS metadata files
    if lower == ".ds_store" || lower == "desktop.ini" || lower == "thumbs.db" {
        return true;
    }
    // Ignore permission testing files
    if name.starts_with(".perm_check") {
        return true;
    }
    false
}

fn filter_event(mut ev: Event) -> Option<Event> {
    // Filter by event kind: only Create, Remove, or Modify(Name)
    let is_wanted_kind = match &ev.kind {
        EventKind::Create(_) => true,
        EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Name(_)) => true,
        _ => false,
    };
    if !is_wanted_kind {
        return None;
    }

    // Filter paths: ignore OS junk and permission probe files; require full perms on target dir
    ev.paths.retain(|p| {
        // ignore names like .DS_Store, desktop.ini, and any starting with .perm_check
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if is_ignored_name(name) {
                return false;
            }
        }
        // Check full permissions on the directory (for files: check parent)
        let dir_to_check: &Path = if p.is_dir() {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        let perms = crate::fs::util::check_dir_permissions(dir_to_check);
        perms.read && perms.write && perms.execute
    });

    if ev.paths.is_empty() {
        // Nothing relevant remains to forward
        None
    } else {
        Some(ev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    // RAII guard that removes the directory tree on drop (even if test panics).
    struct TempDirGuard(PathBuf);
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
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn watch_dir_receives_create_event() {
        let tmp = TempDirGuard::new("fs_watch_create");
        let temp_dir = tmp.path();

        let (_listener, mut rx) = FsListener::watch(temp_dir).expect("should start watcher");

        // Perform a file create inside the watched directory
        let file_path = temp_dir.join("hello.txt");
        fs::write(&file_path, b"hello").unwrap();

        // Wait for at least one event to arrive with an overall timeout
        let overall = Duration::from_secs(10);
        let got_any = tokio::time::timeout(overall, async { rx.recv().await.is_some() })
            .await
            .unwrap_or(false);

        assert!(
            got_any,
            "expected at least one filesystem event after creating a file"
        );

        // Best-effort explicit cleanup (not necessary due to Drop, but harmless)
        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn watch_nonexistent_path_errors() {
        let tmp = TempDirGuard::new("fs_watch_missing");
        let missing = tmp.path().join("subdir_that_does_not_exist");
        // don't create it
        let res = FsListener::watch(&missing);
        assert!(
            res.is_err(),
            "expected error when watching a non-existent path"
        );
    }

    #[tokio::test]
    async fn unwatched_disc_folder_does_not_emit_events() {
        let tmp = TempDirGuard::new("fs_unwatched_disc");
        let temp_dir = tmp.path();

        // Create .disc before starting watcher so FsListener will try to unwatch it
        let disc_dir = temp_dir.join(".disc");
        std::fs::create_dir_all(&disc_dir).unwrap();

        let (_listener, mut rx) = FsListener::watch(temp_dir).expect("should start watcher");

        // Create a file inside the .disc folder which should be unwatched
        let disc_file = disc_dir.join("ignored.txt");
        std::fs::write(&disc_file, b"hi").unwrap();

        // Ensure that no events with paths under .disc are received within the timeout
        let overall = Duration::from_secs(3);
        let saw_disc_event = tokio::time::timeout(overall, async {
            loop {
                match rx.recv().await {
                    Some(ev) => {
                        if ev.paths.iter().any(|p| p.starts_with(&disc_dir)) {
                            return true;
                        }
                        // keep listening until timeout fires
                    }
                    None => return false,
                }
            }
        })
        .await
        .unwrap_or(false);

        assert!(
            !saw_disc_event,
            "should not receive events for files under .disc since it is unwatched"
        );

        let _ = std::fs::remove_file(&disc_file);
    }
}
