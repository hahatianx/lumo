use crate::config::APP_CONFIG;
use crate::err::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub struct Peer {
    pub identifier: String,

    pub peer_name: String,
    pub peer_addr: IpAddr,

    pub is_main: AtomicBool,
    pub is_active: AtomicBool,

    pub last_seen_ms: AtomicU64,
    /// Minutes east of UTC (UTC = 0). Explicitly stores timezone offset for last_seen.
    pub last_seen_tz_offset_minutes: AtomicI32,
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let time = UNIX_EPOCH
            + Duration::from_millis(self.last_seen_ms.load(Ordering::Relaxed))
            + Duration::from_millis(
                self.last_seen_tz_offset_minutes.load(Ordering::Relaxed) as u64 * 60 * 1000,
            );
        let date_time: DateTime<Utc> = time.into();
        write!(
            f,
            "Peer {{ identifier: {}, name: {}, peer_addr: {}, is_main: {}, is_active: {}, last_seen: {} }}",
            &self.identifier,
            &self.peer_name,
            &self.peer_addr,
            &self.is_main.load(Ordering::Relaxed),
            &self.is_active.load(Ordering::Relaxed),
            date_time.format("%Y-%m-%d %H:%M:%S")
        )
    }
}

impl Hash for Peer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identifier.hash(state);
    }
}

impl PartialEq<Self> for Peer {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
    }
}

impl Eq for Peer {}

impl Peer {
    pub fn new(identifier: String, peer_name: String, peer_addr: IpAddr, is_main: bool) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            identifier,
            peer_name,
            peer_addr,
            is_main: AtomicBool::new(is_main),
            is_active: AtomicBool::new(true),
            last_seen_ms: AtomicU64::new(now_ms),
            last_seen_tz_offset_minutes: AtomicI32::new(0),
        }
    }

    /// return true if the peer hasn't expired
    pub async fn peer_valid(&self) -> bool {
        // 1. Read peer expiration from config (seconds). Use non-blocking try_read; fallback to default 60s
        let peer_expires_after_in_sec: u64 = APP_CONFIG.get_peer_expires_after_in_sec().await;

        // 2. Get current UTC time in milliseconds since UNIX_EPOCH
        let now_ms_utc: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // 3. Convert last_seen to UTC using its timezone offset (minutes east of UTC)
        let last_seen_local_ms = self.last_seen_ms.load(Ordering::Relaxed);
        let tz_offset_min = self.last_seen_tz_offset_minutes.load(Ordering::Relaxed);
        let offset_ms: i128 = (tz_offset_min as i128) * 60_000i128; // minutes -> ms

        // local time = UTC + offset => UTC = local - offset
        let last_seen_utc_ms: u64 = if offset_ms >= 0 {
            last_seen_local_ms.saturating_sub(offset_ms as u64)
        } else {
            // negative offset (west of UTC): subtracting a negative => add
            last_seen_local_ms.saturating_add((-offset_ms) as u64)
        };

        let expires_ms = peer_expires_after_in_sec.saturating_mul(1000);
        let valid_until_ms = last_seen_utc_ms.saturating_add(expires_ms);

        valid_until_ms >= now_ms_utc
    }
}

pub struct PeerTable {
    peers: RwLock<HashMap<String, Arc<Peer>>>,
}

impl PeerTable {
    pub fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
        }
    }

    pub async fn remove_peer(&self, peer: Peer) -> Result<()> {
        let mut table = self.peers.write().await;
        if !table.contains_key(&peer.identifier) {
            return Err(format!("Peer {} does not exist", peer.identifier).into());
        }
        table.remove(&peer.identifier);
        Ok(())
    }

    pub async fn update_peer(&self, peer: Peer) -> Result<()> {
        let mut table = self.peers.write().await;
        table.insert(peer.identifier.clone(), Arc::new(peer));
        Ok(())
    }

    pub async fn get_peer(&self, identifier: &str) -> Option<Arc<Peer>> {
        let table = self.peers.read().await;
        match table.get(identifier) {
            Some(peer) => {
                if peer.is_active.load(Ordering::Relaxed) {
                    Some(Arc::clone(peer))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Promote the peer to be the main node
    pub async fn promote_peer(&self, identifier: &str) -> Result<()> {
        let table = self.peers.read().await;
        match table.get(identifier) {
            Some(peer) => {
                if peer.is_active.load(Ordering::Relaxed) {
                    peer.is_main.store(true, Ordering::Relaxed);
                    Ok(())
                } else {
                    Err(format!("Peer {} is inactive", identifier).into())
                }
            }
            None => Err(format!("Peer {} does not exist", identifier).into()),
        }
    }

    /// Refresh the peer's last seen timestamp
    pub async fn refresh_peer(&self, identifier: &str) -> Result<()> {
        let table = self.peers.read().await;
        match table.get(identifier) {
            Some(peer) => {
                if peer.is_active.load(Ordering::Relaxed) {
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    peer.last_seen_ms.store(now_ms, Ordering::Relaxed);
                    // Times are stored in UTC; keep explicit offset (minutes east of UTC)
                    peer.last_seen_tz_offset_minutes.store(0, Ordering::Relaxed);
                    Ok(())
                } else {
                    Err(format!("Peer {} is inactive", identifier).into())
                }
            }
            None => Err(format!("Peer {} does not exist", identifier).into()),
        }
    }

    /// Disables the peer by marking it as inactive
    pub async fn disable_peer(&self, identifier: &str) -> Result<()> {
        let table = self.peers.read().await;
        match table.get(identifier) {
            Some(peer) => {
                if peer.is_active.load(Ordering::Relaxed) {
                    peer.is_active.store(false, Ordering::Relaxed);
                    Ok(())
                } else {
                    Err(format!("Peer {} is inactive", identifier).into())
                }
            }
            None => Err(format!("Peer {} does not exist", identifier).into()),
        }
    }

    pub async fn peer_table_anti_entropy(&self) -> Result<()> {
        // 1) Collect active peers under a short-lived read lock (no .await inside the lock)
        let active_peers: Vec<Arc<Peer>> = {
            let table = self.peers.read().await;
            table
                .values()
                .filter(|p| p.is_active.load(Ordering::Relaxed))
                .cloned()
                .collect()
        };

        // 2) Drop the lock before awaiting. Now check validity and disable as needed.
        for peer in active_peers {
            if !peer.peer_valid().await {
                // We already hold an Arc to the peer; disabling is just an atomic store.
                // This avoids re-locking the table while we might be in an .await chain.
                if peer.is_active.load(Ordering::Relaxed) {
                    peer.is_active.store(false, Ordering::Relaxed);
                }
            }
        }
        Ok(())
    }

    pub async fn get_peers(&self) -> Vec<Arc<Peer>> {
        let table = self.peers.read().await;
        table.values().cloned().collect()
    }
}

impl Debug for PeerTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        loop {
            match self.peers.try_read() {
                Ok(table) => {
                    writeln!(f, "PeerTable {{ peers_len: {} }}", table.len())?;
                    writeln!(f, "START")?;
                    for peer in table.values() {
                        writeln!(f, "\t{:?}", peer)?;
                    }
                    break;
                }
                Err(_) => {}
            }
        }
        write!(f, "END")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EnvVar};
    use crate::global_var::ENV_VAR;
    use serial_test::serial;

    async fn ensure_env_and_set_expiry(secs: u64) {
        if ENV_VAR.get().is_none() {
            // Build a minimal config
            let mut cfg = Config::new();
            cfg.identity.machine_name = "test".into();
            cfg.identity.private_key_loc = "~/.keys/priv".into();
            cfg.identity.public_key_loc = "~/.keys/pub".into();
            cfg.connection.conn_token = "TOKEN".into();
            cfg.app_config.working_dir = "~/ld_work".into();

            let ev = EnvVar::from_config(&cfg).expect("EnvVar::from_config should succeed");
            let _ = ENV_VAR.set(ev); // ignore if already set by other tests
        }
        // Update expiry deterministically
        if let Some(ev) = ENV_VAR.get() {
            ev.app_config
                .write()
                .await
                .update_peer_expires_after_in_sec(secs);
        }
    }

    #[tokio::test]
    #[serial]
    async fn peer_valid_true_with_positive_offset_within_expiry() {
        ensure_env_and_set_expiry(60).await;

        // Simulate last_seen 30s ago in UTC, with local timezone +120 minutes (UTC+2)
        let now_ms_utc: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last_seen_utc_ms = now_ms_utc.saturating_sub(30_000);
        let offset_min: i32 = 120; // +2 hours east of UTC
        let offset_ms: i128 = (offset_min as i128) * 60_000i128;
        let last_seen_local_ms: u64 = (last_seen_utc_ms as i128 + offset_ms) as u64;

        let peer = Peer::new(
            "peer-1".into(),
            String::from("name"),
            "127.0.0.1".parse().unwrap(),
            false,
        );
        peer.last_seen_ms
            .store(last_seen_local_ms, Ordering::Relaxed);
        peer.last_seen_tz_offset_minutes
            .store(offset_min, Ordering::Relaxed);

        assert!(
            peer.peer_valid().await,
            "peer should be valid within 60s window"
        );
    }

    #[tokio::test]
    #[serial]
    async fn peer_valid_false_with_negative_offset_expired() {
        ensure_env_and_set_expiry(60).await;

        // Simulate last_seen 61s ago in UTC, with local timezone -30 minutes (UTC-5)
        let now_ms_utc: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last_seen_utc_ms = now_ms_utc.saturating_sub(61_000);
        let offset_min: i32 = -300; // -5 hours west of UTC
        let offset_ms: i128 = (offset_min as i128) * 60_000i128;
        let last_seen_local_ms_i128: i128 = (last_seen_utc_ms as i128) + offset_ms;
        let last_seen_local_ms: u64 = if last_seen_local_ms_i128 < 0 {
            0
        } else {
            last_seen_local_ms_i128 as u64
        };

        let peer = Peer::new(
            "peer-2".into(),
            String::from("name"),
            "127.0.0.1".parse().unwrap(),
            false,
        );
        peer.last_seen_ms
            .store(last_seen_local_ms, Ordering::Relaxed);
        peer.last_seen_tz_offset_minutes
            .store(offset_min, Ordering::Relaxed);

        assert!(!peer.peer_valid().await, "peer should be expired after 60s");
    }

    #[tokio::test]
    #[serial]
    async fn peer_table_anti_entropy_disables_only_expired_peers() {
        ensure_env_and_set_expiry(60).await;

        let table = PeerTable::new();

        // Setup a valid peer (seen 30s ago, UTC+1)
        let now_ms_utc: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let valid_last_seen_utc = now_ms_utc.saturating_sub(30_000);
        let valid_offset_min = 60; // UTC+1
        let valid_last_seen_local: u64 =
            (valid_last_seen_utc as i128 + (valid_offset_min as i128) * 60_000i128) as u64;

        let mut p1 = Peer::new(
            "valid".into(),
            "p1".into(),
            "127.0.0.1".parse().unwrap(),
            false,
        );
        p1.last_seen_ms
            .store(valid_last_seen_local, Ordering::Relaxed);
        p1.last_seen_tz_offset_minutes
            .store(valid_offset_min, Ordering::Relaxed);
        table.update_peer(p1).await.unwrap();

        // Setup an expired peer (seen 70s ago, UTC-2)
        let expired_last_seen_utc = now_ms_utc.saturating_sub(70_000);
        let expired_offset_min = -120; // UTC-2
        let expired_last_seen_local_i: i128 =
            expired_last_seen_utc as i128 + (expired_offset_min as i128) * 60_000i128;
        let expired_last_seen_local: u64 = if expired_last_seen_local_i < 0 {
            0
        } else {
            expired_last_seen_local_i as u64
        };

        let mut p2 = Peer::new(
            "expired".into(),
            "p2".into(),
            "127.0.0.1".parse().unwrap(),
            false,
        );
        p2.last_seen_ms
            .store(expired_last_seen_local, Ordering::Relaxed);
        p2.last_seen_tz_offset_minutes
            .store(expired_offset_min, Ordering::Relaxed);
        table.update_peer(p2).await.unwrap();

        // Run anti-entropy
        table.peer_table_anti_entropy().await.unwrap();

        // Verify results
        let valid_peer = table.get_peer("valid").await.expect("valid peer not found");
        assert!(valid_peer.is_active.load(Ordering::Relaxed));

        let expired_peer_arc = table
            .peers
            .read()
            .await
            .get("expired")
            .cloned()
            .expect("expired peer should exist");
        assert!(!expired_peer_arc.is_active.load(Ordering::Relaxed));
    }

    #[tokio::test]
    #[serial]
    async fn peer_table_anti_entropy_completes_without_deadlock() {
        ensure_env_and_set_expiry(1).await; // make validation cheap

        let table = PeerTable::new();
        // Insert a bunch of peers to exercise the loop
        for i in 0..50u32 {
            let mut p = Peer::new(
                format!("p-{i}"),
                "n".into(),
                "127.0.0.1".parse().unwrap(),
                false,
            );
            // Make half expired, half valid
            let now_ms_utc: u64 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if i % 2 == 0 {
                // expired: seen 5s ago with 0s expiry
                let last_seen = now_ms_utc.saturating_sub(5_000);
                p.last_seen_ms.store(last_seen, Ordering::Relaxed);
                p.last_seen_tz_offset_minutes.store(0, Ordering::Relaxed);
            } else {
                // valid: seen now
                p.last_seen_ms.store(now_ms_utc, Ordering::Relaxed);
                p.last_seen_tz_offset_minutes.store(0, Ordering::Relaxed);
            }
            table.update_peer(p).await.unwrap();
        }

        // If anti-entropy held the lock across await, this timeout would likely trigger under CI.
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            table.peer_table_anti_entropy(),
        )
        .await;
        assert!(res.is_ok(), "anti-entropy timed out (possible deadlock)");
        // Also ensure we can concurrently read while it runs (no deadlock). Do a quick read now.
        let _ = table.peers.read().await; // should not hang
    }
}
