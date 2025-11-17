use crate::config::EnvVar;
use crate::config::env_var::AppConfig;
use crate::global_var::ENV_VAR;
use std::sync::LazyLock;
use tokio::sync::RwLock;

pub static APP_CONFIG: LazyLock<SharedConfig> = LazyLock::new(|| SharedConfig {});

#[derive(Debug)]
pub struct SharedConfig {}

impl SharedConfig {
    pub fn new(_env_var: &EnvVar) -> Self {
        // Kept for backward compatibility; no longer stores a cloned Arc to avoid stale state.
        SharedConfig {}
    }

    fn get_config(&self) -> &RwLock<AppConfig> {
        // Always resolve through ENV_VAR at call time to avoid initialization order issues.
        ENV_VAR
            .get()
            .expect("ENV_VAR must be initialized before accessing APP_CONFIG")
            .app_config
            .as_ref()
    }

    pub async fn get_working_dir(&self) -> String {
        let config = self.get_config();
        String::from(config.read().await.get_working_dir())
    }

    pub async fn get_peer_expires_after_in_sec(&self) -> u64 {
        let config = self.get_config();
        config.read().await.get_peer_expires_after_in_sec()
    }

    pub async fn update_peer_expires_after_in_sec(&self, new_value: u64) {
        let config = self.get_config();
        config
            .write()
            .await
            .update_peer_expires_after_in_sec(new_value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn app_config_access_and_update_works() {
        // Arrange: build a minimal Config
        let mut cfg = Config::new();
        cfg.identity.machine_name = "test-machine".into();
        cfg.identity.private_key_loc = "~/.keys/priv".into();
        cfg.identity.public_key_loc = "~/.keys/pub".into();
        cfg.connection.conn_token = "CONN_TOKEN".into();
        cfg.app_config.working_dir = "~".into();

        // Initialize ENV_VAR before touching APP_CONFIG
        let env_var = EnvVar::from_config(&cfg).expect("EnvVar::from_config should succeed");
        // It is safe to set once across serial tests
        let _ = ENV_VAR.set(env_var);

        // Act + Assert: default values
        let working_dir = APP_CONFIG.get_working_dir().await;
        let expected_home = std::env::var("HOME").unwrap();
        assert!(working_dir.starts_with(&expected_home));

        let expires = APP_CONFIG.get_peer_expires_after_in_sec().await;
        assert_eq!(
            expires, 60,
            "default peer_expires_after_in_sec should be 60"
        );

        // Update and verify
        APP_CONFIG.update_peer_expires_after_in_sec(120).await;
        let expires_new = APP_CONFIG.get_peer_expires_after_in_sec().await;
        assert_eq!(expires_new, 120);
    }
}
