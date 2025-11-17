use crate::action;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum FileCommands {
    Pull {
        #[arg(short = 'p', long = "peer")]
        peer_identifier: String,

        #[arg(short = 'f', long = "file")]
        file_path: String,

        #[arg(short = 'c', long = "checksum")]
        expected_checksum: Option<u64>,
    },
}

pub fn handle_file_commands(cmd: &FileCommands) {
    match cmd {
        FileCommands::Pull {
            peer_identifier,
            file_path,
            expected_checksum,
        } => action::pull_file::pull_file(
            peer_identifier.clone(),
            file_path.clone(),
            expected_checksum.clone(),
        ),
    }
}
