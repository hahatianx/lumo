use std::net::IpAddr;
use serde::Deserialize;

struct KeySpec {
    private_key_location: String,
    public_key_location: String,
}

struct Identity {

    machine_name: String,
    mac_addr: String,

    key_spec: KeySpec,

}

struct ConnectionConfig {

    conn_token: String,
    port: u16,
    ip_addr: IpAddr,

}

#[derive(Deserialize)]
struct AppConfig {



}

struct EnvVar {

    identity: Identity,

}