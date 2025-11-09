use crate::action;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum PeerCommands {
    /// List known peers
    List,
}

pub fn handle_peer_commands(peer_cmd: &PeerCommands) {
    match peer_cmd {
        PeerCommands::List => {
            action::list_peers::list_peers();
        }
    }
}
