use crate::config::EnvVar;
use crate::network::NetworkSetup;
use crate::tasks::task_queue::TaskQueue;
use crate::utilities::AsyncLogger;
use std::sync::{LazyLock, OnceLock};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub static LOGGER_CELL: OnceLock<AsyncLogger> = OnceLock::new();
pub(crate) static LOGGER: crate::utilities::logger::Logger = crate::utilities::logger::Logger;
pub static ENV_VAR: OnceLock<EnvVar> = OnceLock::new();
pub static GLOBAL_VAR: OnceLock<GlobalVar> = OnceLock::new();
pub static DEBUG_MODE: LazyLock<bool> = LazyLock::new(|| {
    let env_var = std::env::var("DEBUG_MODE").unwrap_or_default();
    let debug_mode = env_var == "1" || env_var == "true";
    debug_mode
});

#[derive(Debug)]
pub struct GlobalVar {
    pub logger_handle: Mutex<Option<JoinHandle<()>>>,

    pub task_queue: Mutex<Option<TaskQueue>>,

    pub network_setup: Mutex<Option<NetworkSetup>>,
}
