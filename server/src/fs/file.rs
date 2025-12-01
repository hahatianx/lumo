use crate::err::Result;
use crate::fs::fs_lock::{LumoFileGuard, RwLock};
use crate::fs::util::{get_relative_path, normalize_path, round_to_fat32};
use std::fmt::Debug;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock as AsyncRwLock;
use xxhash_rust::xxh64::Xxh64;

#[derive(Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Must be guarded by system level file lock
struct FileFingerPrint {
    size: u64,
    mtime: SystemTime,
    checksum: Option<u64>,
}

impl Debug for FileFingerPrint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileFingerPrint")
            .field("size", &self.size)
            .field("mtime", &self.mtime)
            .finish()
    }
}

impl FileFingerPrint {
    pub fn new(size: u64, mtime: SystemTime) -> Self {
        Self {
            size,
            mtime,
            checksum: None,
        }
    }

    pub fn get_checksum(&self, size: u64, mtime: SystemTime) -> Option<u64> {
        if self.size == size && self.mtime == mtime {
            self.checksum
        } else {
            None
        }
    }

    pub fn set_checksum(&mut self, size: u64, mtime: SystemTime, checksum: u64) {
        self.size = size;
        self.mtime = mtime;
        self.checksum = Some(checksum);
    }
}

/// Get file size and mtime. To get the tuple accurately, added a lock to the file.
fn get_file_sz_and_mtime<P: AsRef<Path>>(p: P) -> Result<(u64, SystemTime)> {
    let meta = std::fs::metadata(p)?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .map(round_to_fat32)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    Ok((size, mtime))
}

/// Must be guarded by system level file lock
/// LumoFile stores absolute paths internally; relative paths are derived when needed.
pub struct LumoFile {
    path: PathBuf, // absolute path

    pub size: u64,
    pub mtime: SystemTime,

    fingerprint: AsyncRwLock<FileFingerPrint>,
}

impl LumoFile {
    pub fn rel_path(&self) -> PathBuf {
        // derive relative path from absolute path against working_dir
        get_relative_path(&self.path).unwrap_or_else(|_| self.path.clone())
    }

    pub fn abs_path(&self) -> PathBuf {
        // already absolute; normalize to be safe
        normalize_path(&self.path.to_str().unwrap()).unwrap()
    }
}

impl Debug for LumoFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LumoFile")
            .field("path", &self.path)
            .field("size", &self.size)
            .field("mtime", &self.mtime)
            .field("fingerprint", &self.fingerprint)
            .finish()
    }
}

impl LumoFile {
    /// path can either be a relative or absolute path.
    pub async fn new(path: PathBuf) -> Result<Self> {
        let p: &Path = path.as_ref();
        let full_path = normalize_path(p.to_str().unwrap())?;
        let _guard = RwLock::new(&full_path).write().await?;
        let (size, mtime) = get_file_sz_and_mtime(&full_path)?;
        Ok(Self {
            path: full_path,
            size,
            mtime,
            fingerprint: AsyncRwLock::new(FileFingerPrint::new(size, mtime)),
        })
    }

    /// path must be an absolute path (used for initializing before metadata is known)
    pub fn new_init(path: PathBuf) -> Self {
        Self {
            path,
            size: 0,
            mtime: SystemTime::UNIX_EPOCH,
            fingerprint: AsyncRwLock::new(FileFingerPrint::new(0, SystemTime::UNIX_EPOCH)),
        }
    }

    /// Determine whether two LumoFile instances refer to the same underlying file or to
    /// files with identical content.
    ///
    /// Fast-paths, in order:
    /// - If the paths are byte-for-byte equal, return true.
    /// - If size or mtime differ, return false (avoids checksum work).
    /// - If size and mtime match, compare content checksums.
    ///
    /// Notes:
    /// - The checksum path is comparatively expensive and only used when cheap checks indicate
    ///   a potential match.
    pub async fn same_file(&self, other: &Self) -> bool {
        // 1) Exact path match
        if self.path.as_os_str() == other.path.as_os_str() {
            return true;
        }

        // 2) Quick negative: different size or mtime means different content
        if self.size != other.size || self.mtime != other.mtime {
            return false;
        }

        // 3) Content comparison via checksum as a last resort
        let my_checksum = self.get_checksum().await;
        let other_checksum = other.get_checksum().await;
        if let (Ok(my_checksum), Ok(other_checksum)) = (my_checksum, other_checksum) {
            return my_checksum == other_checksum;
        }
        false
    }

    /// Compute and cache an XXH64 checksum of the file contents.
    ///
    /// Behavior and performance:
    /// - Returns a cached checksum when the stored (size, mtime) match the current
    ///   metadata known to this LumoFile instance.
    /// - Otherwise, acquires a per-path reader lock to avoid concurrent mutation and
    ///   streams the file efficiently in 64 KiB chunks to compute the checksum.
    /// - After computing, updates the fingerprint cache with the metadata observed
    ///   during hashing.
    ///
    /// Errors are wrapped into the crate's Result type.
    pub async fn get_checksum(&self) -> Result<u64> {
        if let Some(checksum) = self
            .fingerprint
            .read()
            .await
            .get_checksum(self.size, self.mtime)
        {
            return Ok(checksum);
        }

        let abs_path = self.abs_path();
        let guard = RwLock::new(&abs_path).read().await?;

        // Compute checksum under exclusive lock; single metadata read is sufficient.
        let (size, mtime, checksum, _guard) = get_file_checksum(guard).await?;

        // Update cached fingerprint to reflect the observed metadata when checksum was computed.
        self.fingerprint
            .write()
            .await
            .set_checksum(size, mtime, checksum);

        Ok(checksum)
    }
}

// to call this function, you need to acquire a read/write lock on the path
pub async fn get_file_checksum<G: LumoFileGuard>(guard: G) -> Result<(u64, SystemTime, u64, G)> {
    // fetch file descriptor from guard
    let mut file = &*guard;
    let position = file.seek(SeekFrom::Current(0))?;
    file.seek(SeekFrom::Start(0))?;

    let (size, mtime) = {
        let meta = file.metadata()?;
        (
            meta.len(),
            meta.modified()
                .ok()
                .map(round_to_fat32)
                .unwrap_or(SystemTime::UNIX_EPOCH),
        )
    };

    // Stream the file into the hasher
    let mut hasher = Xxh64::new(0);
    let mut buf = vec![0u8; 64 * 1024];
    let mut processed: usize = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        processed += n;
        #[cfg(debug_assertions)]
        if processed >= (4 * 1024 * 1024) {
            // In debug/test builds, yield periodically to make long reads observable by tests
            // that check for system-level locking during checksum calculation.
            tokio::task::yield_now().await;
            processed = 0;
        }
    }
    let checksum = hasher.digest();
    file.seek(SeekFrom::Start(position))?;
    Ok((size, mtime, checksum, guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EnvVar};
    use crate::global_var::ENV_VAR;
    use std::time::Duration;
    use xxhash_rust::xxh64::xxh64;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        p.push(format!(
            "{}_{}_{}_{}",
            "filetracker",
            std::process::id(),
            ts,
            name
        ));
        p
    }

    fn create_env_var() {
        if ENV_VAR.get().is_none() {
            let mut cfg = Config::new();
            cfg.identity.machine_name = "test-machine".into();
            cfg.identity.private_key_loc = "~/.ssh/id_rsa".into();
            cfg.identity.public_key_loc = "~/.ssh/id_rsa.pub".into();
            cfg.connection.conn_token = "TOKEN".into();
            cfg.app_config.working_dir = "/".into();
            let ev = EnvVar::from_config(&cfg).unwrap();
            let _ = ENV_VAR.set(ev);
        }
    }

    #[tokio::test]
    async fn checksum_basic_and_cache() {
        let p = temp_path("basic.txt");
        std::fs::write(&p, b"hello world").unwrap();

        let expected = xxh64(b"hello world", 0);

        let tracker = LumoFile::new(p.clone()).await.unwrap();
        let c1 = tracker.get_checksum().await.expect("checksum ok");
        assert_eq!(c1, expected);

        // Call again to hit the cache path: should return same value
        let c2 = tracker.get_checksum().await.expect("checksum ok");
        assert_eq!(c2, expected);

        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn checksum_updates_on_change() {
        let p = temp_path("change.txt");
        std::fs::write(&p, b"first").unwrap();

        let tracker = LumoFile::new(p.clone()).await.unwrap();
        let c1 = tracker.get_checksum().await.unwrap();

        // Ensure mtime changes on some filesystems (coarse granularity)
        tokio::time::sleep(Duration::from_millis(1100)).await;
        std::fs::write(&p, b"second").unwrap();

        // Use a fresh tracker to also verify metadata is read correctly
        let tracker2 = LumoFile::new(p.clone()).await.unwrap();
        let c2 = tracker2.get_checksum().await.unwrap();
        assert_ne!(c1, c2, "checksum should change after content update");

        // Verify expected value
        let expected2 = xxh64(b"second", 0);
        assert_eq!(c2, expected2);

        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_checksum_uses_lock() {
        let p = temp_path("concurrent.txt");
        let content = vec![42u8; 256 * 1024];
        std::fs::write(&p, &content).unwrap();
        let expected = xxh64(&content, 0);

        // Spawn multiple concurrent checksum computations within the same task.
        let p1 = p.clone();
        let p2 = p.clone();
        let p3 = p.clone();
        let p4 = p.clone();
        let p5 = p.clone();
        let p6 = p.clone();
        let p7 = p.clone();
        let p8 = p.clone();
        let f1 = async move {
            LumoFile::new(p1)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f2 = async move {
            LumoFile::new(p2)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f3 = async move {
            LumoFile::new(p3)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f4 = async move {
            LumoFile::new(p4)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f5 = async move {
            LumoFile::new(p5)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f6 = async move {
            LumoFile::new(p6)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f7 = async move {
            LumoFile::new(p7)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let f8 = async move {
            LumoFile::new(p8)
                .await
                .unwrap()
                .get_checksum()
                .await
                .unwrap()
        };
        let (r1, r2, r3, r4, r5, r6, r7, r8) = tokio::join!(f1, f2, f3, f4, f5, f6, f7, f8);
        for v in [r1, r2, r3, r4, r5, r6, r7, r8] {
            assert_eq!(v, expected);
        }

        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn bench_get_checksum_unit() {
        // This is a lightweight, unit-test-style benchmark meant to run under `cargo test`.
        // It measures wall-clock time for get_checksum across a few file sizes.
        use std::time::Instant;

        fn temp_path_local(name: &str) -> PathBuf {
            let mut p = std::env::temp_dir();

            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            p.push(format!(
                "{}_{}_{}_{}",
                "filetracker_bench",
                std::process::id(),
                ts,
                name
            ));
            p
        }

        async fn run_once(path: &Path) -> u64 {
            let t = LumoFile::new(path.to_path_buf()).await.expect("new ok");
            t.get_checksum().await.expect("checksum ok")
        }

        // Prepare test files of various sizes
        let sizes = [1_usize << 10, 1_usize << 20, 8_usize << 20]; // 1 KiB, 1 MiB, 8 MiB
        let mut files: Vec<(usize, PathBuf)> = Vec::new();
        for s in sizes {
            let p = temp_path_local(&format!("bench_{}", s));

            if ENV_VAR.get().is_none() {
                let mut cfg = Config::new();
                cfg.identity.machine_name = "test-machine".into();
                cfg.identity.private_key_loc = "~/.ssh/id_rsa".into();
                cfg.identity.public_key_loc = "~/.ssh/id_rsa.pub".into();
                cfg.connection.conn_token = "TOKEN".into();
                cfg.app_config.working_dir = p.parent().unwrap().to_str().unwrap().to_string();
                let ev = EnvVar::from_config(&cfg).unwrap();
                let _ = ENV_VAR.set(ev);
            }

            let mut f = std::fs::File::create(&p).unwrap();
            let chunk = vec![0xCDu8; 64 * 1024];
            let mut remaining = s;
            while remaining > 0 {
                let to_write = remaining.min(chunk.len());
                use std::io::Write;
                f.write_all(&chunk[..to_write]).unwrap();
                remaining -= to_write;
            }
            files.push((s, p));
        }

        // Warmup: compute once per file to populate caches/metadata
        for (_, p) in &files {
            let _ = run_once(p).await;
        }

        // Benchmark loop: N iterations per file
        let iters = 5u32;
        for (size, p) in &files {
            let mut best = Duration::from_secs(u64::MAX);
            let mut total = Duration::from_millis(0);
            for _ in 0..iters {
                let start = Instant::now();
                let _ = run_once(p).await;
                let elapsed = start.elapsed();
                if elapsed < best {
                    best = elapsed;
                }
                total += elapsed;
            }
            let avg = total / iters;
            let mib = (*size as f64) / (1024.0 * 1024.0);
            let avg_mibs = mib / (avg.as_secs_f64());
            let best_mibs = mib / (best.as_secs_f64());
            eprintln!(
                "unit-bench: size={} bytes avg={:?} best={:?} avg_throughput={:.2} MiB/s best_throughput={:.2} MiB/s",
                size, avg, best, avg_mibs, best_mibs
            );
        }

        // Cleanup
        for (_, p) in files {
            let _ = std::fs::remove_file(p);
        }
    }
}
