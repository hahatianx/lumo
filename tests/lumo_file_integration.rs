use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
use server::fs::LumoFile;
use tokio::time::sleep;

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
        std::fs::create_dir_all(&p).unwrap();
        TempDirGuard(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Helper: create a temporary unique path inside a guarded temp directory
fn temp_path_in(tmp: &TempDirGuard, name: &str) -> PathBuf {
    let mut p = tmp.path().to_path_buf();
    p.push(name);
    p
}

async fn write_file_with_size(path: &Path, size: usize, byte: u8) {
    use tokio::io::AsyncWriteExt;
    let mut f = tokio::fs::File::create(path).await.unwrap();
    let chunk = vec![byte; 64 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let to_write = remaining.min(chunk.len());
        f.write_all(&chunk[..to_write]).await.unwrap();
        remaining -= to_write;
    }
    f.flush().await.unwrap();
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_is_locked_during_checksum_read() {
    let tmp = TempDirGuard::new("lumo_integ");
    // Create a moderately large file to keep checksum busy for a short time.
    let path = temp_path_in(&tmp, "locked_while_read.bin");
    let size = 64usize * 1024 * 1024; // 64 MiB
    write_file_with_size(&path, size, 0xAB).await;

    // Run a non-Send task on a LocalSet to avoid requiring Send
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            // Start checksum in a separate local task
            let p1 = path.clone();
            let checksum_task = tokio::task::spawn_local(async move {
                let lf = LumoFile::new(p1).await.unwrap();
                lf.get_checksum().await.unwrap()
            });

            // Give the checksum task a head start to acquire the read lock
            sleep(Duration::from_millis(20)).await;

            // Attempt to acquire an exclusive system lock while read is ongoing; it should fail.
            // We'll poll for a short duration and expect all attempts to fail while checksum runs.
            let start = std::time::Instant::now();
            let mut saw_lock_failure_during_read = false;
            while start.elapsed() < Duration::from_millis(150) {
                let file = std::fs::File::open(&path).unwrap();
                let res = file.try_lock_exclusive();
                match res {
                    Ok(_) => {
                        // If we somehow acquired the lock, release and note unexpected success
                        let _ = file.unlock();
                    }
                    Err(_) => {
                        saw_lock_failure_during_read = true;
                        break;
                    }
                }
                // Brief pause before trying again
                sleep(Duration::from_millis(5)).await;
            }
            assert!(
                saw_lock_failure_during_read,
                "exclusive system lock should fail while checksum holds the read lock"
            );

            // Now wait for checksum to complete
            let _sum = checksum_task.await.unwrap();

            // After read finishes, exclusive lock should be obtainable
            let file = std::fs::File::open(&path).unwrap();
            let got = file.try_lock_exclusive();
            assert!(
                got.is_ok(),
                "exclusive lock should succeed after checksum completes"
            );
            let _ = file.unlock();
        })
        .await;
}

#[serial_test::serial]
#[tokio::test]
async fn checksum_matches_state_at_size_and_mtime() {
    use xxhash_rust::xxh64::xxh64;

    let tmp = TempDirGuard::new("lumo_integ");
    let path = temp_path_in(&tmp, "state_checksum.txt");

    // Initial content A
    let content_a = vec![42u8; 256 * 1024];
    tokio::fs::write(&path, &content_a).await.unwrap();

    // Build LumoFile and compute checksum S1
    let lf_a = LumoFile::new(path.clone()).await.unwrap();
    let s1 = lf_a.get_checksum().await.unwrap();
    let expected_s1 = xxh64(&content_a, 0);
    assert_eq!(s1, expected_s1, "checksum should match initial content A");

    // Record the observed metadata snapshot (size, mtime) from the instance
    let size_a = lf_a.size;
    let mtime_a = lf_a.mtime;

    // Modify the file: change content and ensure mtime changes beyond FAT32 rounding.
    // Write different content B and then sleep to cross a 2s boundary.
    let content_b = vec![7u8; 128 * 1024];
    tokio::fs::write(&path, &content_b).await.unwrap();

    // Ensure mtime difference beyond coarse rounding (sleep a bit)
    sleep(Duration::from_millis(2100)).await;
    // Touch the file to update mtime
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(&path);
    // Append 1 byte so size changes too
    {
        use tokio::io::AsyncWriteExt;
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .await
            .unwrap()
            .write_all(&[0xFF])
            .await
            .unwrap();
    }

    // A fresh LumoFile view should pick up new size/mtime and recompute checksum
    let lf_b = LumoFile::new(path.clone()).await.unwrap();
    let s2 = lf_b.get_checksum().await.unwrap();

    // The checksum should reflect the current bytes on disk
    let mut expected_current = content_b.clone();
    expected_current.push(0xFF);
    let expected_s2 = xxh64(&expected_current, 0);
    assert_eq!(s2, expected_s2, "checksum should follow modified content B");

    // And the earlier snapshot remains associated with its original (size, mtime)
    assert_eq!(
        size_a as usize,
        content_a.len(),
        "size in snapshot matches A"
    );
    assert!(
        mtime_a <= lf_b.mtime,
        "mtime should have advanced after modification"
    );
}
