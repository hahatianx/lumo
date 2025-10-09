use crate::config::EnvVar;
use crate::utilities::AsyncLogger;
use std::sync::OnceLock;
use tokio::task::JoinHandle;

pub static LOGGER: OnceLock<AsyncLogger> = OnceLock::new();
pub static ENV_VAR: OnceLock<EnvVar> = OnceLock::new();

pub struct GlobalVar {
    pub logger_handle: JoinHandle<()>,
}
