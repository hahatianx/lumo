use notify::EventKind;
use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
use server::config::{Config, EnvVar};
use server::fs::init_fs_index;
use std::fs;
use std::time::Duration;

// Simple temp dir guard for integration tests
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
#[serial_test::serial]
async fn fs_index_integration_on_file_event_flow() {
    let tmp = TempDirGuard::new("integ_fs_index_flow");
    let p = tmp.path().join("k.bin");
    write_bytes(&p, 2048, 0xAA).await;

    // Initialize logger and other FS deps in this temp dir
    let (logger, _task) = server::fs::init_working_dir(tmp.path()).await.unwrap();
    let _ = server::global_var::LOGGER_CELL.set(logger);

    // Initialize env_var
    let mut config = Config::new();
    config.identity.machine_name = String::from("integration_test");
    let env_var = EnvVar::from_config(&config).unwrap();
    let _ = server::global_var::ENV_VAR.set(env_var);

    // Using the global FS_INDEX singleton
    use server::fs::FS_INDEX;
    init_fs_index().await.expect("TODO: panic message");

    // Create
    FS_INDEX
        .on_file_event(&p, EventKind::Create(CreateKind::File))
        .await
        .unwrap();

    let present = FS_INDEX.with_entry(&p, |_| ()).await.is_some();
    assert!(present);

    // Modify content to mark stale
    {
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .await
            .unwrap();
        f.write_all(&[9, 9, 9, 9]).await.unwrap();
        f.flush().await.unwrap();
    }
    FS_INDEX
        .on_file_event(&p, EventKind::Modify(ModifyKind::Data(DataChange::Content)))
        .await
        .unwrap();
    let stale = FS_INDEX.with_entry(&p, |e| e.needs_rescan()).await.unwrap();
    assert!(stale);

    // Run anti-entropy rescan
    FS_INDEX.index_stale_rescan().await.unwrap();
    let s2 = FS_INDEX.with_entry(&p, |e| e.needs_rescan()).await.unwrap();
    assert!(!s2);

    // Remove underlying file and notify Remove
    std::fs::remove_file(&p).unwrap();
    let _ = FS_INDEX
        .on_file_event(&p, EventKind::Remove(RemoveKind::File))
        .await;
    let gone = FS_INDEX.with_entry(&p, |_| ()).await.is_none();
    assert!(gone);
}

#[tokio::test]
#[serial_test::serial]
async fn fs_index_integration_concurrent_events_no_deadlock() {
    let tmp = TempDirGuard::new("integ_fs_index_concurrent");

    // Initialize logger and other FS deps in this temp dir
    let (logger, _task) = server::fs::init_working_dir(tmp.path()).await.unwrap();
    let _ = server::global_var::LOGGER_CELL.set(logger);

    // Initialize env_var
    let mut config = Config::new();
    config.identity.machine_name = String::from("integration_test");
    let env_var = EnvVar::from_config(&config).unwrap();
    let _ = server::global_var::ENV_VAR.set(env_var);

    // Using the global FS_INDEX singleton
    use server::fs::FS_INDEX;
    init_fs_index().await.expect("TODO: panic message");

    let mut paths = Vec::new();
    for i in 0..4 {
        let p = tmp.path().join(format!("c{}.bin", i));
        write_bytes(&p, 1024 + i * 10, 0x10 + i as u8).await;
        paths.push(p);
    }

    // Fire creates concurrently
    let mut handles = Vec::new();
    for p in paths.clone() {
        let h = tokio::spawn(async move {
            FS_INDEX
                .on_file_event(&p, EventKind::Create(CreateKind::File))
                .await
                .unwrap();
        });
        handles.push(h);
    }
    // Wait with timeout to detect deadlocks
    tokio::time::timeout(Duration::from_secs(5), async {
        for h in handles {
            h.await.unwrap();
        }
    })
    .await
    .expect("create events timed out (deadlock)");

    // Fire modify concurrently too
    let mut handles2 = Vec::new();
    for p in paths.clone() {
        let h = tokio::spawn(async move {
            FS_INDEX
                .on_file_event(&p, EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .await
                .unwrap();
        });
        handles2.push(h);
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        for h in handles2 {
            h.await.unwrap();
        }
    })
    .await
    .expect("modify events timed out (deadlock)");

    // Best-effort cleanup: remove files from disk and tell index
    for p in paths {
        let _ = std::fs::remove_file(&p);
        let _ = FS_INDEX
            .on_file_event(&p, EventKind::Remove(RemoveKind::File))
            .await;
    }
}
