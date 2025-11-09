use crate::action;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum LocalFileCommands {
    /// Pull a file from local server by path
    Pull {
        /// Source file path to pull
        #[arg(short = 'p', long = "path")]
        path: String,
        /// Optional expected checksum (u64)
        #[arg(short = 'c', long = "checksum")]
        checksum: Option<u64>,
    },
}

pub fn handle_local_file_commands(cmd: &LocalFileCommands) {
    match cmd {
        LocalFileCommands::Pull { path, checksum } => {
            action::local_pull_file::local_pull_file(&path, *checksum);
        }
    }
}
