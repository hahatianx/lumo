use crate::err::{Error, Result};
use crate::fs::util::normalize_path;
use crate::lumo_error;
use fs2::FileExt;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::{File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};
use tokio::time::sleep;

/// wrapped by a file lock OS call
/// There must be at most one FileLockGuard towards each file in the system
#[derive(Debug)]
pub(crate) struct FileLockGuard {
    inner: File,
}

impl FileLockGuard {
    pub fn new<P: AsRef<Path>>(path: P, exclusive: bool) -> Result<Self> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.as_ref())?;
        if exclusive {
            f.try_lock_exclusive()?;
        } else {
            f.try_lock_shared()?;
        }
        Ok(Self { inner: f })
    }
}

impl Deref for FileLockGuard {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for FileLockGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        // Best-effort cleanup; ignore errors.
        let _ = self.inner.unlock();
    }
}

/// Acquire an exclusive, cross-process file lock via system-level locking.
/// This function retries with exponential backoff for a bounded duration.
pub(crate) async fn acquire_lock(path: &Path, exclusive: bool) -> Result<FileLockGuard> {
    let mut backoff_ms: u64 = 10;
    let max_backoff_ms: u64 = 500;
    let max_attempts: u32 = 100; // ~ up to a few seconds total

    if !path.exists() {
        return Err(format!("File lock path does not exist: {}", path.display()).into());
    }

    let mut last_err: Option<Error> = None;

    for _ in 0..max_attempts {
        match FileLockGuard::new(&path, exclusive) {
            Ok(guard) => return Ok(guard),
            Err(e) => {
                last_err = Some(e);
                sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = std::cmp::min(backoff_ms * 2, max_backoff_ms);
            }
        }
    }
    Err(format!(
        "timeout acquiring file lock for '{}' (last_err={})",
        path.display(),
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    )
    .into())
}

#[derive(Debug)]
struct InnerState {
    // Tracks active readers in this process
    read_count: AtomicUsize,
    // Holds the system-level exclusive file lock while there is at least one reader
    sys_guard: Option<Arc<FileLockGuard>>,
}

// In-process per-path async RWLock registry with system-level exclusivity for the read phase
#[derive(Debug)]
struct PerPathState {
    rw: Arc<TokioRwLock<()>>,
    // Serializes initialization of the first reader (acquiring system lock)
    init: TokioMutex<()>,
    state: Mutex<InnerState>,
}

impl PerPathState {
    fn new() -> Self {
        Self {
            rw: Arc::new(TokioRwLock::new(())),
            init: TokioMutex::new(()),
            state: Mutex::new(InnerState {
                read_count: AtomicUsize::new(0),
                sys_guard: None,
            }),
        }
    }
}

static RW_REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<PerPathState>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<PerPathState>>> {
    RW_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_or_create_lock(path: &Path) -> Arc<PerPathState> {
    let key = PathBuf::from(path);
    let mut map = registry().lock().unwrap();
    if let Some(lock) = map.get(&key) {
        return lock.clone();
    }
    let arc = Arc::new(PerPathState::new());
    map.insert(key, arc.clone());
    arc
}

/// A per-path async RWLock that allows multiple concurrent readers in-process,
/// but enforces system-level exclusivity (single holder across processes) during reads and writes.
pub struct RwLock {
    path: PathBuf,
    inner: Arc<PerPathState>,
}

impl RwLock {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let full_path = normalize_path(path.as_ref().to_str().unwrap()).unwrap();
        let p = full_path.as_ref();
        let inner = get_or_create_lock(p);
        Self {
            path: p.to_path_buf(),
            inner,
        }
    }

    fn open_file(&self, path: &Path) -> Result<File> {
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| lumo_error!("Failed to open file with read lock: {}", e).into())
    }

    /// Acquire a read lock. Multiple readers are allowed concurrently in-process.
    /// System-wide, we take a single exclusive lock for the duration of the first-to-last reader.
    pub async fn read(&self) -> Result<ReadGuard> {
        // Take the in-process read guard first
        let guard = self.inner.rw.clone().read_owned().await;

        // Fast path: if there are existing readers in this process, just bump the counter
        {
            let state_guard =
                self.inner.state.lock().map_err(|_| {
                    lumo_error!("File lock state poisoned while acquiring read guard")
                })?;
            if state_guard.read_count.load(Ordering::Acquire) > 0 {
                let file_lock = state_guard.sys_guard.as_ref().unwrap().clone();
                state_guard.read_count.fetch_add(1, Ordering::AcqRel);
                let f = self.open_file(&self.path)?;
                return Ok(ReadGuard {
                    _guard: guard,
                    state: self.inner.clone(),
                    file_lock,
                    file: f,
                });
            }
        }

        // Slow path: become the first reader; ensure only one task initializes
        let _init = self.inner.init.lock().await;
        // Decide if we are the first reader without holding the state mutex across await
        let need_first;
        {
            let state_guard = self.inner.state.lock().map_err(|_| {
                lumo_error!("File lock state poisoned while initializing first reader")
            })?;
            if state_guard.read_count.load(Ordering::Acquire) == 0 {
                need_first = true;
            } else {
                let file_lock = state_guard.sys_guard.as_ref().unwrap().clone();
                state_guard.read_count.fetch_add(1, Ordering::AcqRel);
                let f = self.open_file(&self.path)?;
                return Ok(ReadGuard {
                    _guard: guard,
                    state: self.inner.clone(),
                    file_lock,
                    file: f,
                });
            }
        }

        if need_first {
            // Acquire the system-level exclusive lock without holding the state mutex
            let sys = Arc::new(acquire_lock(&self.path, false).await?);
            // Install the sys lock and set first reader count
            let mut state_guard = self.inner.state.lock().map_err(|_| {
                lumo_error!("File lock state poisoned while initializing first reader")
            })?;
            // Since we still hold the init mutex, no other first-reader can race us.
            debug_assert_eq!(state_guard.read_count.load(Ordering::Relaxed), 0);
            state_guard.sys_guard = Some(sys.clone());
            state_guard.read_count.store(1, Ordering::Release);
            let f = self.open_file(&self.path)?;
            return Ok(ReadGuard {
                _guard: guard,
                state: self.inner.clone(),
                file_lock: sys,
                file: f,
            });
        }

        unreachable!("Do not reach here. Should have returned early")
    }

    /// Acquire a write lock. This is exclusive in-process and also guards cross-process
    /// by taking the sidecar file lock for the target path.
    pub async fn write(&self) -> crate::err::Result<WriteGuard> {
        let lockfile = acquire_lock(&self.path, true).await?;
        let guard = self.inner.rw.clone().write_owned().await;
        Ok(WriteGuard {
            _guard: guard,
            file_lock: lockfile,
        })
    }
}

/// Guard returned by RwLock::read()
#[derive(Debug)]
pub struct ReadGuard {
    _guard: OwnedRwLockReadGuard<()>,
    state: Arc<PerPathState>,
    file_lock: Arc<FileLockGuard>,
    file: File,
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        // Decrement local reader count; release system lock if this was the last reader
        if let Ok(mut st) = self.state.state.lock() {
            if st.read_count.fetch_sub(1, Ordering::AcqRel) == 1 {
                // Dropping the guard releases the system-level lock
                st.sys_guard = None;
            }
        }
    }
}

impl Deref for ReadGuard {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

/// Guard returned by RwLock::write()
#[derive(Debug)]
pub struct WriteGuard {
    _guard: OwnedRwLockWriteGuard<()>,
    file_lock: FileLockGuard,
}

impl Deref for WriteGuard {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.file_lock.inner
    }
}

impl DerefMut for WriteGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file_lock.inner
    }
}

pub trait LumoFileGuard: Send + Debug + Deref<Target = File> {}

impl LumoFileGuard for ReadGuard {}
impl LumoFileGuard for WriteGuard {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn unique_temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("local_disc_lock_test_{}_{}", name, nanos));
        // Ensure file exists for locking
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&p)
            .expect("create temp file");
        p
    }

    #[tokio::test]
    async fn concurrent_reads_and_write_blocking() {
        let path = unique_temp_path("reads_block_write");
        let lock = Arc::new(RwLock::new(&path));

        let n = 5usize;
        let mut handles = Vec::new();
        let (tx_all, mut rx_all) = tokio::sync::mpsc::channel::<()>(n);

        for _ in 0..n {
            let lock_cloned = lock.clone();
            let tx = tx_all.clone();
            handles.push(tokio::spawn(async move {
                let _g = lock_cloned.read().await.expect("read lock");
                // Signal acquired
                let _ = tx.send(()).await;
                // Hold for a bit
                sleep(Duration::from_millis(200)).await;
                // drop guard at end of scope
            }));
        }
        drop(tx_all);

        // Wait until all readers have acquired (or time out)
        for _ in 0..n {
            timeout(Duration::from_secs(1), rx_all.recv())
                .await
                .expect("timeout waiting readers started")
                .expect("channel closed");
        }

        // A write should be blocked while readers are active
        let write_attempt = timeout(Duration::from_millis(150), async {
            let _wg = lock.write().await.expect("write lock");
        })
        .await;
        assert!(
            write_attempt.is_err(),
            "write should time out while readers active"
        );

        // Wait for readers to finish
        for h in handles {
            h.await.expect("reader task join");
        }

        // Now write should succeed quickly
        timeout(Duration::from_secs(1), async {
            let _wg = lock.write().await.expect("write after readers");
        })
        .await
        .expect("write should succeed after readers");
    }

    #[tokio::test]
    async fn write_exclusive_against_reads_and_writes() {
        let path = unique_temp_path("write_exclusive");
        let lock = Arc::new(RwLock::new(&path));

        // Hold write lock
        let wg = lock.write().await.expect("write lock first");

        // Reader should block
        let lock_r = lock.clone();
        let read_block = timeout(Duration::from_millis(150), async move {
            let _rg = lock_r.read().await.expect("read lock");
        })
        .await;
        assert!(read_block.is_err(), "read should block while write held");

        // Another writer should also block
        let lock_w = lock.clone();
        let write_block = timeout(Duration::from_millis(150), async move {
            let _wg2 = lock_w.write().await.expect("write lock 2");
        })
        .await;
        assert!(
            write_block.is_err(),
            "second write should block while first write held"
        );

        drop(wg);

        // After releasing, both read and write should succeed
        timeout(Duration::from_secs(1), async {
            let _rg = lock.read().await.expect("read after write");
        })
        .await
        .expect("read should succeed after write release");

        timeout(Duration::from_secs(1), async {
            let _wg = lock.write().await.expect("write after write");
        })
        .await
        .expect("write should succeed after write release");
    }

    #[tokio::test]
    async fn cancellation_during_first_reader_init_does_not_leak() {
        let path = unique_temp_path("cancel_first_reader");
        let lock = Arc::new(RwLock::new(&path));

        // Pre-lock the file to force the reader into the slow-path waiting for system lock
        let sys_guard = acquire_lock(&path, false).await.expect("pre-lock file");

        // Start a reader that will get stuck trying to acquire system lock
        let mut handle = tokio::spawn({
            let lock = lock.clone();
            async move {
                let _rg = lock.read().await.expect("read lock start");
                // If we got here, system lock was acquired — but we expect cancellation before that
                // Hold briefly
                sleep(Duration::from_millis(50)).await;
            }
        });

        // Give the task a moment to reach acquire_lock
        sleep(Duration::from_millis(50)).await;

        // Abort the task (cancellation)
        handle.abort();
        let _ = handle.await; // ignore JoinError

        // Drop the pre-held system lock
        drop(sys_guard);

        // Now a write should succeed promptly; if anything leaked it would block
        timeout(Duration::from_secs(1), async {
            let _wg = lock.write().await.expect("write after cancel");
        })
        .await
        .expect("write should succeed; no leaked locks after cancellation");
    }
}
