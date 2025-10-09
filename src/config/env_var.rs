use crate::config::config::Config;
use crate::err::Result;
use crate::network::get_private_ipv4_with_mac;
use std::net::IpAddr;

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
    ip_addr: IpAddr,
}

#[derive(Debug)]
struct AppConfig {}

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
                mac_addr: mac_addr.map(|u| format!("{:02X}", u)).join(":"),
                key_spec: KeySpec {
                    private_key_location: config.identity.private_key_loc.clone(),
                    public_key_location: config.identity.public_key_loc.clone(),
                },
            },
            connection: ConnectionConfig {
                conn_token: config.connection.conn_token.clone(),
                port: config.connection.port,
                ip_addr: IpAddr::V4(ipv4),
            },
            app_config: AppConfig {},
        })
    }
}
