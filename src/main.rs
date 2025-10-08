use crate::config::{Opts, get_or_create_config};

mod config;
mod err;

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
        }
        Err(e) => {
            eprintln!("Failed to load or create configuration: {}", e);
            std::process::exit(1);
        }
    }
}
