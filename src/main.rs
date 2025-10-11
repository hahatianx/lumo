use bytes::Bytes;
use crate::config::Config;
use crate::config::{EnvVar, Opts, get_or_create_config};
use crate::err::Result;
use crate::fs::init_fs;
use crate::global_var::{ENV_VAR, GLOBAL_VAR, GlobalVar, LOGGER, LOGGER_CELL};
use crate::network::{init_network, terminate_network};
use crate::network::protocol::messages::HelloMessage;
use crate::network::protocol::protocol::Protocol;
use crate::tasks::{init_core, shutdown_core};

mod config;
mod err;
mod fs;
mod global_var;
mod network;
mod tasks;
mod utilities;

fn print_version_and_exit() -> ! {
    // These are set by build.rs; fall back to unknown if missing
    let pkg_version = env!("CARGO_PKG_VERSION");
    let commit = option_env!("GIT_COMMIT").unwrap_or("unknown");
    let state = option_env!("GIT_STATE").unwrap_or("unknown");
    let built = option_env!("BUILD_TIME").unwrap_or("unknown time");
    println!(
        "server {} (commit: {}, state: {}, built: {})",
        pkg_version, commit, state, built
    );
    std::process::exit(0)
}

async fn init(config: &Config) -> Result<()> {
    // Start server initialization
    // 1. Read config
    //   1.0. Test config validation
    //   1.1. Set up environment variables
    // 2. Set up a working directory
    //   2.0. Test directory permissions READ | WRITE | EXECUTE
    //   2.1. Fail if working the directory does not exist
    //   2.2. Get or create .server director
    //   2.3. Set up external logger
    // 3. Set up network initialization
    // 4. File system initialization

    // panic on failures

    let env_var = EnvVar::from_config(config).expect("Failed to set environment variables");

    let (logger, logger_handle) = init_fs(env_var.get_working_dir())
        .await
        .expect("Failed to initialize logger");

    ENV_VAR
        .set(env_var)
        .expect("Environment variable already set");
    LOGGER_CELL.set(logger).expect("Logger already set");

    // LOGGER enabled starting from this point

    let task_queue = match init_core().await {
        Ok(task_queue) => {
            task_queue
        },
        Err(e) => {
            LOGGER.error(format!("Failed to initialize task queue: {}", e));
            panic!("Failed to initialize task queue");
        }
    };

    let network_setup = match init_network(&task_queue).await {
        Ok(network_setup) => {
            network_setup
        },
        Err(e) => {
            LOGGER.error(format!("Failed to initialize network: {}", e));
            panic!("Failed to initialize network");
        }
    };

    let global_var = GlobalVar {
        logger_handle: tokio::sync::Mutex::new(Some(logger_handle)),
        task_queue: tokio::sync::Mutex::new(Some(task_queue)),
        network_setup: tokio::sync::Mutex::new(Some(network_setup)),
    };

    GLOBAL_VAR
        .set(global_var)
        .expect("Global variable already set");

    Ok(())
}

async fn system_shutdown() {
    LOGGER.info("System shutting down...");
    if let Some(gv) = GLOBAL_VAR.get() {

        if let Some(ns) = gv.network_setup.lock().await.take() {
            let _ = terminate_network(ns).await;
        }


        if let Some(tq) = gv.task_queue.lock().await.take() {
            let _ = shutdown_core(tq).await;
        }

        // shutdown logger
        LOGGER.shutdown().await;
        if let Some(handle) = gv.logger_handle.lock().await.take() {
            let _ = handle.await;
        }
    }
}

#[tokio::main]
async fn main() {
    let opts = Opts::from_args();

    if opts.version {
        print_version_and_exit();
    }

    // Always provide Some("") as a fallback default path for interactive setup
    let cfg_path_opt: Option<&str> = opts
        .config
        .as_deref()
        .map(|p| p.to_str().unwrap_or(""))
        .or(Some(""));

    match get_or_create_config(cfg_path_opt) {
        Ok(config) => {
            dbg!(&config);
            init(&config).await.unwrap();
            dbg!(ENV_VAR.get().unwrap());
        }
        Err(e) => {
            panic!("Failed to load or create configuration: {}", e);
        }
    }

    loop {

        let hello_message = HelloMessage::new(
            "127.0.0.1".to_string(),
            8080,
            String::from("Alice"),
            String::from(ENV_VAR.get().unwrap().get_conn_token()),
            0,
        );

        let bytes = Bytes::from(hello_message.serialize());
        let sender = &GLOBAL_VAR.get().unwrap().network_setup.lock().await.as_ref().unwrap().sender.sender();

        let _ = sender.broadcast(bytes).await;

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    system_shutdown().await;
}
