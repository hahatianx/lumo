use crate::config::config::Config;
use crate::err::Result;
use crate::fs::util::expand_tilde;
use crate::network::get_private_ipv4_with_mac;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug)]
struct KeySpec {
    private_key_location: String,
    public_key_location: String,
}

#[derive(Debug)]
struct Identity {
    machine_name: String,
    mac_addr: String,

    key_spec: KeySpec,
}

#[derive(Debug)]
struct ConnectionConfig {
    conn_token: String,
    port: u16,
    file_sync_port: u16,
    ip_addr: IpAddr,
}

#[derive(Debug)]
struct AppConfig {
    working_dir: String,
}

#[derive(Debug)]
pub struct EnvVar {
    identity: Identity,
    connection: ConnectionConfig,
    app_config: AppConfig,
}

impl EnvVar {
    pub fn from_config(config: &Config) -> Result<Self> {
        let (ipv4, mac_addr) = get_private_ipv4_with_mac().unwrap();

        Ok(Self {
            identity: Identity {
                machine_name: config.identity.machine_name.clone(),
                mac_addr: mac_addr.map(|u| format!("{:02x}", u)).join(":"),
                key_spec: KeySpec {
                    private_key_location: expand_tilde(&config.identity.private_key_loc),
                    public_key_location: expand_tilde(&config.identity.public_key_loc),
                },
            },
            connection: ConnectionConfig {
                conn_token: config.connection.conn_token.clone(),
                port: 14514, // reserved port for server to listen on protocol messages
                file_sync_port: 11451, // reserved port for file sync
                ip_addr: IpAddr::V4(ipv4),
            },
            app_config: AppConfig {
                working_dir: expand_tilde(&config.app_config.working_dir),
            },
        })
    }

    pub fn get_working_dir(&self) -> &str {
        &self.app_config.working_dir
    }
    pub fn get_conn_token(&self) -> &str {
        &self.connection.conn_token
    }
    pub fn get_port(&self) -> u16 {
        self.connection.port
    }
    pub fn get_ip_addr(&self) -> IpAddr {
        self.connection.ip_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config::Config;
    use serial_test::serial;
    use std::env;

    #[test]
    #[serial]
    fn envvar_from_config_expands_tilde_and_preserves_fields() {
        // Arrange: set HOME to a unique temporary directory
        let expected_home = env::var("HOME").unwrap();

        // Build a minimal config with tilde paths
        let mut cfg = Config::new();
        cfg.identity.machine_name = "machine".into();
        cfg.identity.private_key_loc = "~/.keys/priv".into();
        cfg.identity.public_key_loc = "~/.keys/pub".into();
        cfg.connection.conn_token = "TOKEN123".into();
        cfg.app_config.working_dir = "~/workdir".into();

        // Act
        let ev = EnvVar::from_config(&cfg).expect("from_config should succeed");

        // Assert: getters
        assert_eq!(ev.get_conn_token(), "TOKEN123");
        assert_eq!(ev.get_port(), 14514);
        assert_eq!(
            ev.get_working_dir(),
            format!("{}/{}", expected_home, "workdir")
        );

        // Assert: internal fields are expanded as well (same module, so we can access privates)
        assert!(
            ev.identity
                .key_spec
                .private_key_location
                .starts_with(&expected_home)
        );
        assert!(
            ev.identity
                .key_spec
                .public_key_location
                .starts_with(&expected_home)
        );
    }
}
