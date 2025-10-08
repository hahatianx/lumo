use structopt::StructOpt;
use crate::config::{get_or_create_config, Config, Opts};

mod config;
mod err;

#[tokio::main]
async fn main() {
    let opts = Opts::from_args();

    match get_or_create_config(opts.config.to_str()) {
        Ok(config) => {
            dbg!(&config);
        }
        Err(e) => {
            eprintln!("Failed to load or create configuration: {}", e);
            std::process::exit(1);
        }
    }
}
