use crate::err::Result;
use crate::fs::LumoFile;
use crate::global_var::ENV_VAR;
use crate::global_var::LOGGER;
use notify::EventKind;
use notify::event::ModifyKind;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock as AsyncRwLock;
use crate::fs::fs_lock::RwLock;

/// A single file entry tracked by the in-memory index.
///
/// Notes:
/// - This struct is not Sync by itself; all concurrent access is protected by the
///   FileIndex's AsyncRwLock.
pub struct FileEntry {
    file: LumoFile,
    last_writer: Option<String>,
    is_active: bool,
    is_stale: bool,

    last_modified: SystemTime,
}

impl Debug for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileEntry")
            .field("file", &self.file)
            .field("last_writer", &self.last_writer)
            .field("is_active", &self.is_active)
            .field("is_stale", &self.is_stale)
            .finish()
    }
}

impl FileEntry {
    pub fn new(file: LumoFile) -> Self {
        Self {
            file,
            last_writer: None,
            is_active: true,
            is_stale: false,
            last_modified: SystemTime::now(),
        }
    }

    pub fn with_last_writer(mut self, writer: impl Into<String>) -> Self {
        self.last_writer = Some(writer.into());
        self
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
        self.last_modified = SystemTime::now();
    }

    pub fn set_stale(&mut self, stale: bool) {
        self.is_stale = stale;
        self.last_modified = SystemTime::now();
    }

    pub fn mark_stale(&mut self) {
        self.is_stale = true;
        self.last_modified = SystemTime::now();
    }

    pub fn needs_rescan(&self) -> bool {
        self.is_stale
    }

    pub fn set_last_writer(&mut self, writer: impl Into<String>) {
        self.last_writer = Some(writer.into());
        self.last_modified = SystemTime::now();
    }
}

#[derive(Default)]
struct FileIndexInner {
    // Store entries as Arc<RwLock<>> so tasks can hold references and update without locking the whole index
    map: HashMap<PathBuf, std::sync::Arc<AsyncRwLock<FileEntry>>>,
    // Cached metadata for maintaining indices without awaiting on per-entry locks
    meta: HashMap<PathBuf, (u64, SystemTime)>,
    by_size: HashMap<u64, HashSet<PathBuf>>, // size -> paths
    by_size_mtime: HashMap<(u64, u64), HashSet<PathBuf>>, // (size, mtime_secs) -> paths
    // Track which paths are currently active so we can answer contains_key synchronously
    active_paths: HashSet<PathBuf>,
}

impl FileIndexInner {
    fn mtime_key(mtime: SystemTime) -> u64 {
        mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn contains_key<P: AsRef<Path>>(&self, path: P) -> bool {
        let p = path.as_ref();
        if !self.map.contains_key(p) {
            return false;
        }
        self.active_paths.contains(p)
    }

    fn insert_indices(&mut self, path: &Path, size: u64, mtime: SystemTime) {
        // by_size
        self.by_size
            .entry(size)
            .or_default()
            .insert(path.to_path_buf());
        // by_size_mtime
        let mk = Self::mtime_key(mtime);
        self.by_size_mtime
            .entry((size, mk))
            .or_default()
            .insert(path.to_path_buf());
    }

    fn remove_indices(&mut self, path: &Path, size: u64, mtime: SystemTime) {
        if let Some(set) = self.by_size.get_mut(&size) {
            set.remove(path);
            if set.is_empty() {
                self.by_size.remove(&size);
            }
        }
        let mk = Self::mtime_key(mtime);
        if let Some(set) = self.by_size_mtime.get_mut(&(size, mk)) {
            set.remove(path);
            if set.is_empty() {
                self.by_size_mtime.remove(&(size, mk));
            }
        }
    }

    pub async fn debug(&self) -> String {
        let mut s = String::new();
        LOGGER.debug(format!("FileIndexInner::debug, table len: {}", self.map.len()).as_str());
        for (k, v) in &self.map {
            let e = v.read().await;
            if let Some((sz, mt)) = self.meta.get(k).cloned() {
                writeln!(s, "{}: {:?}, meta=({}, {:?})", k.display(), e, sz, mt).unwrap();
            } else {
                writeln!(s, "{}: {:?}", k.display(), e).unwrap();
            }
        }
        s
    }
}

/// Async-ready index of files with secondary indices to help find same-file candidates.
///
/// Concurrency model:
/// - Uses a single Tokio RwLock protecting all maps to keep updates atomic and avoid deadlocks.
#[derive(Default)]
pub struct FileIndex {
    inner: AsyncRwLock<FileIndexInner>,
}

impl FileIndex {
    pub fn new() -> Self {
        Self {
            inner: AsyncRwLock::new(FileIndexInner::default()),
        }
    }

    // Helper: fetch entry Arc for a given path under read lock.
    async fn get_entry_arc(&self, p: &Path) -> Option<std::sync::Arc<AsyncRwLock<FileEntry>>> {
        let guard = self.inner.read().await;
        guard.map.get(p).cloned()
    }

    // Helper: mutate an entry if present, applying the provided closure.
    async fn with_entry_mut<P: AsRef<Path>, T>(
        &self,
        path: P,
        f: impl FnOnce(&mut FileEntry) -> T,
    ) -> Option<T> {
        let p = path.as_ref().to_path_buf();
        if let Some(arc) = self.get_entry_arc(&p).await {
            let mut e = arc.write().await;
            Some(f(&mut *e))
        } else {
            None
        }
    }

    /// Insert or replace the entry for the file's path.
    pub async fn upsert(&self, entry: FileEntry) {
        let key = entry.file.path.clone();
        let size = entry.file.size;
        let mtime = entry.file.mtime;
        let is_active = entry.is_active;
        let arc_entry = std::sync::Arc::new(AsyncRwLock::new(entry));
        let mut guard = self.inner.write().await;
        // Remove old indices if existed
        if let Some((old_size, old_mtime)) = guard.meta.remove(&key) {
            guard.remove_indices(&key, old_size, old_mtime);
        }
        // Insert/replace entry and indices
        guard.map.insert(key.clone(), arc_entry);
        guard.meta.insert(key.clone(), (size, mtime));
        guard.insert_indices(&key, size, mtime);
        // Update active cache
        if is_active {
            guard.active_paths.insert(key.clone());
        } else {
            guard.active_paths.remove(&key);
        }
    }

    /// Insert if absent. Returns true if inserted, false if already existed.
    pub async fn insert_if_absent(&self, entry: FileEntry) -> bool {
        let key = entry.file.path.clone();
        let size = entry.file.size;
        let mtime = entry.file.mtime;
        let is_active = entry.is_active;
        let arc_entry = std::sync::Arc::new(AsyncRwLock::new(entry));
        let mut guard = self.inner.write().await;
        if !guard.map.contains_key(&key) {
            guard.insert_indices(&key, size, mtime);
            guard.meta.insert(key.clone(), (size, mtime));
            guard.map.insert(key.clone(), arc_entry);
            if is_active {
                guard.active_paths.insert(key);
            } else {
                guard.active_paths.remove(&key);
            }
            true
        } else {
            false
        }
    }

    /// Remove an entry by path. Returns true if an entry was removed.
    pub async fn remove<P: AsRef<Path>>(&self, path: P) -> bool {
        let p = path.as_ref();
        let mut guard = self.inner.write().await;
        let existed = if let Some((size, mtime)) = guard.meta.remove(p) {
            guard.remove_indices(p, size, mtime);
            true
        } else {
            false
        };
        // Remove the entry (if any) from the map as well
        let _ = guard.map.remove(p);
        // Remove from active cache
        guard.active_paths.remove(p);
        existed
    }

    /// Check whether an entry exists for the given path.
    pub async fn contains<P: AsRef<Path>>(&self, path: P) -> bool {
        let guard = self.inner.read().await;
        guard.map.contains_key(path.as_ref())
    }

    /// The number of entries currently indexed.
    pub async fn len(&self) -> usize {
        let guard = self.inner.read().await;
        guard.map.len()
    }

    /// Get cached metadata (size, mtime) for a path from the index without locking the entry.
    pub(crate) async fn get_meta<P: AsRef<Path>>(&self, path: P) -> Option<(u64, SystemTime)> {
        let guard = self.inner.read().await;
        guard.meta.get(path.as_ref()).cloned()
    }

    /// Mark a path as active or inactive.
    pub async fn set_active<P: AsRef<Path>>(&self, path: P, active: bool) -> Result<()> {
        let p = path.as_ref().to_path_buf();
        // Only update entry and cache if the entry actually exists to avoid cache pollution
        if self.get_entry_arc(&p).await.is_some() {
            let _ = self.with_entry_mut(&p, |e| e.set_active(active)).await;
            let mut guard = self.inner.write().await;
            if active {
                guard.active_paths.insert(p);
            } else {
                guard.active_paths.remove(&p);
            }
            Ok(())
        } else {
            // Ensure we don't keep stale cache entries for non-existent paths
            let mut guard = self.inner.write().await;
            guard.active_paths.remove(&p);
            Err(format!("path not found in index: {}", p.display()).into())
        }
    }

    /// Mark a path as stale or fresh.
    pub async fn set_stale<P: AsRef<Path>>(&self, path: P, stale: bool) -> Result<()> {
        if self
            .with_entry_mut(path.as_ref(), |e| e.set_stale(stale))
            .await
            .is_some()
        {
            Ok(())
        } else {
            Err(format!("path not found in index: {}", path.as_ref().display()).into())
        }
    }

    /// Update the last_writer for a path.
    pub async fn set_last_writer<P: AsRef<Path>>(
        &self,
        path: P,
        writer: impl Into<String>,
    ) -> Result<()> {
        let writer = writer.into();
        if self
            .with_entry_mut(path.as_ref(), |e| e.set_last_writer(writer))
            .await
            .is_some()
        {
            Ok(())
        } else {
            Err(format!("path not found in index: {}", path.as_ref().display()).into())
        }
    }

    /// Read-only access pattern: apply a closure to the entry if it exists and
    /// return its computed value. The closure must not perform async work.
    pub async fn with_entry<P, T>(&self, path: P, f: impl FnOnce(&FileEntry) -> T) -> Option<T>
    where
        P: AsRef<Path>,
    {
        let p = path.as_ref().to_path_buf();
        let arc = {
            let guard = self.inner.read().await;
            guard.map.get(&p).cloned()
        };
        if let Some(entry) = arc {
            let e = entry.read().await;
            Some(f(&*e))
        } else {
            None
        }
    }

    /// List all paths currently in the index.
    pub(crate) async fn list_paths(&self) -> Vec<PathBuf> {
        let guard = self.inner.read().await;
        guard.map.keys().cloned().collect()
    }

    /// Find candidate paths that could refer to the same file based on size.
    pub(crate) async fn candidates_by_size(&self, size: u64) -> Vec<PathBuf> {
        let guard = self.inner.read().await;
        guard
            .by_size
            .get(&size)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Find candidate paths that could refer to the same file based on (size, mtime).
    pub(crate) async fn candidates_by_size_mtime(
        &self,
        size: u64,
        mtime: SystemTime,
    ) -> Vec<PathBuf> {
        let mk = FileIndexInner::mtime_key(mtime);
        let guard = self.inner.read().await;
        guard
            .by_size_mtime
            .get(&(size, mk))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Shortcut to get candidates for a given LumoFile, preferring the narrower key (size, mtime).
    pub async fn candidates_for(&self, file: &LumoFile) -> Vec<PathBuf> {
        let mut v = self.candidates_by_size_mtime(file.size, file.mtime).await;
        if v.is_empty() {
            v = self.candidates_by_size(file.size).await;
        }
        v
    }

    pub async fn debug(&self) -> String {
        let guard = self.inner.read().await;
        guard.debug().await
    }
}

/// Interface level for a file entry in the index.
/// Plan:
/// Take notify events CREATE | REMOVE | MODIFY_NAME | MODIFY_CONTENT events
///
/// Ideally
/// > CREATE: add the new file into index
/// > REMOVE: mark the file index not active
/// > MODIFY_NAME: check file path existence:
/// > if not exist: it's a move from, mark file index inactive
/// > if exists: it's a move destination,
/// > try to find the source file index, if found, move the source index to new path
/// > otherwise create a new file index
/// > MODIFY_CONTENT: mark the file index stale
///
/// Backend job to periodically rescan stale indices
/// Backend job to periodically clean up inactive indices --> a grace period
///
/// Taking notify events disorder into consideration, I take a naive but effective approach:
///   On any event, if the file is not found in the index, treat it as a move from.
///   If the file is found in the index, treat it as a move destination.
///
impl FileIndex {
    async fn on_add<P: AsRef<Path>>(&self, p: P, lf: LumoFile) -> Result<()> {
        self.upsert(FileEntry::new(lf).with_last_writer(ENV_VAR.get().unwrap().get_machine_name()))
            .await;
        Ok(())
    }

    async fn on_remove<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        self.set_active(p, false).await
    }

    async fn on_modify_content<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        self.set_stale(p, true).await
    }

    pub async fn on_file_event<P: AsRef<Path>>(&self, p: P, ek: EventKind) -> Result<()> {
        LOGGER.debug(format!("on_file_event: {} {:?}", p.as_ref().display(), ek));
        match LumoFile::new(p.as_ref().to_path_buf()).await {
            Ok(lf) => {
                // Case 1: found a file
                match ek {
                    EventKind::Create(_) => self.on_add(p, lf).await,
                    EventKind::Remove(_) => Err(format!(
                        "Ignore removing event as it comes from event disorder: {}",
                        p.as_ref().display()
                    )
                    .into()),
                    EventKind::Modify(ModifyKind::Name(_)) => {
                        // Case 1.1: file name changed: treat it as move
                        self.on_add(p, lf).await
                    }
                    EventKind::Modify(ModifyKind::Data(_)) => self.on_modify_content(p).await,
                    _ => {
                        LOGGER.debug(
                            format!("Ignore event {:?} from  {}", ek, p.as_ref().display())
                                .as_str(),
                        );
                        Err(format!("Ignore event {:?} from  {}", ek, p.as_ref().display()).into())
                    }
                }
            }
            Err(e) => {
                // Case 2: file not found: treat it as move
                LOGGER.debug(format!(
                    "Assume the file {} deleted from path, reason: {}",
                    p.as_ref().display(),
                    e
                ));
                self.on_remove(p).await
            }
        }
    }
}

impl FileIndex {

    pub async fn index_stale_rescan(&self) -> Result<()> {
        // Snapshot active entries to avoid holding the index lock while doing I/O
        let entries: Vec<(PathBuf, std::sync::Arc<AsyncRwLock<FileEntry>>)> = {
            let guard = self.inner.read().await;
            guard
                .active_paths
                .iter()
                .filter_map(|pb| {
                    let p = pb.as_path();
                    guard.map.get(p).cloned().map(|arc| (pb.clone(), arc))
                })
                .collect()
        };

        for (path, arc) in entries {
            // Read current flags cheaply
            let (is_active, is_stale ) = {
                let e = arc.read().await;
                (e.is_active, e.is_stale)
            };

            if !is_active {
                continue;
            }

            // If marked stale, refresh metadata and clear the stale flag
            if is_stale {
                match LumoFile::new(path.clone()).await {
                    Ok(lf) => {
                        let mut entry = FileEntry::new(lf);
                        // Ensure flags
                        entry.set_active(true);
                        entry.set_stale(false);
                        // Upsert will refresh indices/meta atomically
                        self.upsert(entry).await;
                        LOGGER.trace(format!("index_anti_entropy: refreshed '{}'", path.display()))
                    }
                    Err(e) => {
                        // This is not expected to happen, but if it does, we should log it
                        LOGGER.error(format!(
                            "index_anti_entropy: failed to refresh '{}': {}",
                            path.display(), e
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn index_inactive_clean(&self) -> Result<()> {
        // Goal: remove entries that have been inactive for more than 10 minutes.
        // Concurrency rules to avoid deadlocks:
        // - Do NOT hold the index write lock while awaiting on per-entry locks.
        // - Snapshot candidate Arcs under a read lock, then inspect entries outside the index lock.
        // - Before removal, re-check inactivity using only index data (active_paths) to avoid
        //   mixing lock orders with per-entry locks.
        let now = SystemTime::now();
        let max_inactive = std::time::Duration::from_secs(10 * 60);

        // Snapshot inactive entries: collect (PathBuf, Arc<Entry>) for paths not in active_paths.
        let inactive_entries: Vec<(PathBuf, std::sync::Arc<AsyncRwLock<FileEntry>>)> = {
            let guard = self.inner.read().await;
            guard
                .map
                .iter()
                .filter_map(|(p, arc)| {
                    if guard.active_paths.contains(p) {
                        None
                    } else {
                        Some((p.clone(), arc.clone()))
                    }
                })
                .collect()
        };

        // Determine which ones are expired based on last_modified timestamp stored in the entry.
        let mut expired: Vec<PathBuf> = Vec::new();
        for (path, arc) in inactive_entries {
            // Read the entry without holding the index lock.
            let e = arc.read().await;
            if !e.is_active {
                if let Ok(elapsed) = now.duration_since(e.last_modified) {
                    if elapsed >= max_inactive {
                        expired.push(path);
                    }
                }
            }
        }

        // Remove expired entries atomically under a write lock to avoid races with concurrent updates.
        for path in expired {
            // Acquire a write lock for re-validation and removal as a single atomic block.
            let mut guard = self.inner.write().await;
            // Re-validate under the write lock: entry exists and is still inactive.
            let still_inactive = guard.map.contains_key(&path) && !guard.active_paths.contains(&path);
            if still_inactive {
                // Remove indices and caches inline to avoid re-entrant locking.
                if let Some((size, mtime)) = guard.meta.remove(&path) {
                    guard.remove_indices(&path, size, mtime);
                }
                // Remove the entry from the map and active cache.
                let _ = guard.map.remove(&path);
                guard.active_paths.remove(&path);
                LOGGER.trace(format!(
                    "index_inactive_clean: removed inactive entry '{}" ,
                    path.display()
                ));
            }
            // write lock dropped here at end of scope iteration
        }
        Ok(())
    }

}

pub static FS_INDEX: LazyLock<FileIndex> = LazyLock::new(|| FileIndex::new());

pub fn init_fs_index() -> Result<&'static FileIndex> {
    Ok(&FS_INDEX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
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

    async fn write_bytes<P: AsRef<std::path::Path>>(p: P, size: usize, byte: u8) {
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::File::create(p.as_ref()).await.unwrap();
        let chunk = vec![byte; 64 * 1024];
        let mut remaining = size;
        while remaining > 0 {
            let to_write = remaining.min(chunk.len());
            f.write_all(&chunk[..to_write]).await.unwrap();
            remaining -= to_write;
        }
        f.flush().await.unwrap();
    }

    #[tokio::test]
    async fn upsert_contains_list_and_flags() {
        let tmp = TempDirGuard::new("fs_index_upsert_contains_list_and_flags");
        let p1 = tmp.path().join("a.bin");
        let p2 = tmp.path().join("b.bin");
        write_bytes(&p1, 1024, 0x11).await;
        write_bytes(&p2, 2048, 0x22).await;

        let lf1 = LumoFile::new(p1.clone()).await.unwrap();
        let lf2 = LumoFile::new(p2.clone()).await.unwrap();

        let index = FileIndex::new();
        index.upsert(FileEntry::new(lf1)).await;
        index.upsert(FileEntry::new(lf2)).await;

        assert!(index.contains(&p1).await);
        assert!(index.contains(&p2).await);
        assert_eq!(index.len().await, 2);

        let mut paths = index.list_paths().await;
        paths.sort();
        let mut expected = vec![p1.clone(), p2.clone()];
        expected.sort();
        assert_eq!(paths, expected);

        // Flags and last_writer updates via per-entry lock
        index.set_active(&p1, true).await;
        index.set_stale(&p1, true).await;
        index.set_last_writer(&p1, "worker-1").await;

        let (active, stale, writer) = index
            .with_entry(&p1, |e| (e.is_active, e.is_stale, e.last_writer.clone()))
            .await
            .unwrap();
        assert!(active);
        assert!(stale);
        assert_eq!(writer.as_deref(), Some("worker-1"));
    }

    #[tokio::test]
    async fn candidates_by_size_and_remove_updates_indices() {
        let tmp = TempDirGuard::new("fs_index_candidates_by_size_and_remove_updates_indices");
        let p1 = tmp.path().join("c1.bin");
        let p2 = tmp.path().join("c2.bin");
        // Same size, different contents
        write_bytes(&p1, 4096, 0xAA).await;
        write_bytes(&p2, 4096, 0xBB).await;

        let lf1 = LumoFile::new(p1.clone()).await.unwrap();
        let lf2 = LumoFile::new(p2.clone()).await.unwrap();

        let index = FileIndex::new();
        index.upsert(FileEntry::new(lf1)).await;
        index.upsert(FileEntry::new(lf2)).await;

        let mut candidates = index.candidates_by_size(4096).await;
        candidates.sort();
        let mut expected = vec![p1.clone(), p2.clone()];
        expected.sort();
        assert_eq!(candidates, expected);

        // Remove one and ensure indices updated
        assert!(index.remove(&p1).await);
        let candidates_after = index.candidates_by_size(4096).await;
        assert_eq!(candidates_after, vec![p2.clone()]);

        // Removing again returns false
        assert!(!index.remove(&p1).await);
    }

    #[tokio::test]
    async fn candidates_by_size_mtime_and_candidates_for_with_hardlink() {
        let tmp =
            TempDirGuard::new("fs_index_candidates_by_size_mtime_and_candidates_for_with_hardlink");
        let p1 = tmp.path().join("samefile_src.bin");
        let p2 = tmp.path().join("samefile_hardlink.bin");
        write_bytes(&p1, 10 * 1024, 0x5C).await;

        // Create a hard link to ensure identical inode and metadata (size, mtime)
        std::fs::hard_link(&p1, &p2).unwrap();

        let lf1 = LumoFile::new(p1.clone()).await.unwrap();
        let lf2 = LumoFile::new(p2.clone()).await.unwrap();
        let size2 = lf2.size;
        let mtime2 = lf2.mtime;

        // Sanity: expect same rounded mtime and size
        assert_eq!(lf1.size, size2);
        assert_eq!(
            FileIndexInner::mtime_key(lf1.mtime),
            FileIndexInner::mtime_key(mtime2)
        );

        let index = FileIndex::new();
        index.upsert(FileEntry::new(lf1)).await;
        index.upsert(FileEntry::new(lf2)).await;

        let mut by_size = index.candidates_by_size(size2).await;
        by_size.sort();
        assert_eq!(by_size, {
            let mut v = vec![p1.clone(), p2.clone()];
            v.sort();
            v
        });

        let mut by_sm = index.candidates_by_size_mtime(size2, mtime2).await;
        by_sm.sort();
        assert_eq!(by_sm, {
            let mut v = vec![p1.clone(), p2.clone()];
            v.sort();
            v
        });

        let mut for_f2 = index
            .candidates_for(&LumoFile::new(p2.clone()).await.unwrap())
            .await;
        for_f2.sort();
        assert_eq!(for_f2, {
            let mut v = vec![p1.clone(), p2.clone()];
            v.sort();
            v
        });
    }

    #[tokio::test]
    async fn insert_if_absent_behaves_as_expected() {
        let tmp = TempDirGuard::new("fs_index_insert_if_absent_behaves_as_expected");
        let p = tmp.path().join("d.bin");
        write_bytes(&p, 1234, 0x01).await;
        let lf = LumoFile::new(p.clone()).await.unwrap();
        let index = FileIndex::new();

        let first = index.insert_if_absent(FileEntry::new(lf)).await;
        assert!(first);

        // Second insert with same path should return false
        let lf_again = LumoFile::new(p.clone()).await.unwrap();
        let second = index.insert_if_absent(FileEntry::new(lf_again)).await;
        assert!(!second);

        // Upsert should keep it present and indices intact
        assert!(index.contains(&p).await);
        let size = 1234u64;
        let v = index.candidates_by_size(size).await;
        assert_eq!(v, vec![p.clone()]);

        // Touch file to ensure mtime changes, then upsert updates indices accordingly
        // Sleeping a bit to cross FAT32 rounding boundary and ensuring mtime update
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let mut file = fs::OpenOptions::new().append(true).open(&p).unwrap();
        file.write_all(&[0xFF]).unwrap();
        drop(file);

        let lf_updated = LumoFile::new(p.clone()).await.unwrap();
        index.upsert(FileEntry::new(lf_updated)).await;

        // Size index should now reflect new size (1235)
        assert!(index.candidates_by_size(1234).await.is_empty());
        assert_eq!(index.candidates_by_size(1235).await, vec![p.clone()]);
    }

    #[tokio::test]
    async fn meta_access_and_updates() {
        let tmp = TempDirGuard::new("fs_index_meta_access_and_updates");
        let p = tmp.path().join("m.bin");
        write_bytes(&p, 777, 0x33).await;

        let lf = LumoFile::new(p.clone()).await.unwrap();
        let size0 = lf.size;
        let mtime0 = lf.mtime;

        let index = FileIndex::new();
        index.insert_if_absent(FileEntry::new(lf)).await;

        let meta0 = index.get_meta(&p).await.unwrap();
        assert_eq!(meta0.0, size0);
        assert_eq!(
            FileIndexInner::mtime_key(meta0.1),
            FileIndexInner::mtime_key(mtime0)
        );

        // Modify file to change size and mtime
        tokio::time::sleep(Duration::from_millis(2100)).await;
        {
            use tokio::io::AsyncWriteExt;
            let mut f = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&p)
                .await
                .unwrap();
            f.write_all(&[0x01]).await.unwrap();
            f.flush().await.unwrap();
        }
        let lf2 = LumoFile::new(p.clone()).await.unwrap();
        let size1 = lf2.size;
        let mtime1 = lf2.mtime;
        index.upsert(FileEntry::new(lf2)).await;

        let meta1 = index.get_meta(&p).await.unwrap();
        assert_eq!(meta1.0, size1);
        assert_eq!(
            FileIndexInner::mtime_key(meta1.1),
            FileIndexInner::mtime_key(mtime1)
        );
        assert!(meta1.0 > meta0.0);
        assert!(FileIndexInner::mtime_key(meta1.1) >= FileIndexInner::mtime_key(meta0.1));
    }
}

#[cfg(test)]
mod more_fs_index_set_active_tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    struct TempDirGuard2(std::path::PathBuf);
    impl TempDirGuard2 {
        fn new(prefix: &str) -> Self {
            let mut p = std::env::temp_dir();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            p.push(format!("{}_{}_{}", prefix, std::process::id(), ts));
            fs::create_dir_all(&p).unwrap();
            TempDirGuard2(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDirGuard2 {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn write_bytes2<P: AsRef<std::path::Path>>(p: P, size: usize, byte: u8) {
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::File::create(p.as_ref()).await.unwrap();
        let chunk = vec![byte; 16 * 1024];
        let mut remaining = size;
        while remaining > 0 {
            let to_write = remaining.min(chunk.len());
            f.write_all(&chunk[..to_write]).await.unwrap();
            remaining -= to_write;
        }
        f.flush().await.unwrap();
    }

    #[tokio::test]
    async fn set_active_returns_err_for_missing_and_does_not_block_future_insert() {
        let tmp = TempDirGuard2::new("fs_index_set_active_err");
        let p = tmp.path().join("x.bin");
        // Ensure file exists on disk but not in index yet
        write_bytes2(&p, 32, 0xEE).await;

        let index = FileIndex::new();
        // Calling set_active before inserting should error
        let err = index.set_active(&p, true).await.err();
        assert!(err.is_some());

        // Now insert the file; insert_if_absent should succeed (no cache pollution prevented it)
        let lf = LumoFile::new(p.clone()).await.unwrap();
        let inserted = index.insert_if_absent(FileEntry::new(lf)).await;
        assert!(inserted);

        // After present, set_active should succeed and mark flag
        index.set_active(&p, true).await.unwrap();
        let is_active = index.with_entry(&p, |e| e.is_active).await.unwrap();
        assert!(is_active);
    }

    #[tokio::test]
    async fn set_active_flips_state_and_is_consistent() {
        let tmp = TempDirGuard2::new("fs_index_set_active_flip");
        let p = tmp.path().join("y.bin");
        write_bytes2(&p, 64, 0xAB).await;

        let lf = LumoFile::new(p.clone()).await.unwrap();
        let index = FileIndex::new();
        index.upsert(FileEntry::new(lf)).await;

        index.set_active(&p, true).await.unwrap();
        let (a1, ts1) = index
            .with_entry(&p, |e| (e.is_active, e.last_modified))
            .await
            .unwrap();
        assert!(a1);

        // Sleep to ensure last_modified changes when flipping
        tokio::time::sleep(Duration::from_millis(10)).await;
        index.set_active(&p, false).await.unwrap();
        let (a2, ts2) = index
            .with_entry(&p, |e| (e.is_active, e.last_modified))
            .await
            .unwrap();
        assert!(!a2);
        assert!(ts2 >= ts1);
    }
}
