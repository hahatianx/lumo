mod action;
mod cli;
mod error;

use crate::cli::local_file::LocalFileCommands;
use crate::cli::peer::PeerCommands;
use crate::cli::task::TaskCommands;
use clap::{Parser, Subcommand};
use crate::cli::file::FileCommands;

#[derive(Debug, Parser)]
#[command(
    name = "local-disc-client",
    version,
    about = "Local-Disc client CLI",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(name = "peer", about = "Peer related commands")]
    Peer {
        #[command(subcommand)]
        command: PeerCommands,
    },
    #[command(name = "task", about = "Task related commands")]
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    #[command(name = "local-file", about = "File related commands (local debug only)")]
    LocalFile {
        #[command(subcommand)]
        command: LocalFileCommands,
    },
    #[command(name = "file", about = "File related commands")]
    File {
        #[command(subcommand)]
        command: FileCommands,
    }
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Peer { command } => cli::peer::handle_peer_commands(command),
        Commands::Task { command } => cli::task::handle_task_commands(command),
        Commands::LocalFile { command } => cli::local_file::handle_local_file_commands(command),
        Commands::File { command } => cli::file::handle_file_commands(command),
    }
}
