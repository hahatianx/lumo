use crate::config::EnvVar;
use crate::utilities::AsyncLogger;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use crate::network::NetworkSetup;
use crate::tasks::task_queue::TaskQueue;

pub static LOGGER_CELL: OnceLock<AsyncLogger> = OnceLock::new();
pub(crate) static LOGGER: crate::utilities::logger::Logger = crate::utilities::logger::Logger;
pub static ENV_VAR: OnceLock<EnvVar> = OnceLock::new();
pub static GLOBAL_VAR: OnceLock<GlobalVar> = OnceLock::new();

#[derive(Debug)]
pub struct GlobalVar {
    pub logger_handle: Mutex<Option<JoinHandle<()>>>,

    pub task_queue:  Mutex<Option<TaskQueue>>,

    pub network_setup: Mutex<Option<NetworkSetup>>,
}
