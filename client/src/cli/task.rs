
use clap::Subcommand;
use crate::action;

#[derive(Debug, Subcommand)]
pub enum TaskCommands {
    List,
}

pub fn handle_task_commands(cmd: &TaskCommands) {
    match cmd {
        TaskCommands::List => {
            unimplemented!()
        }
    }
}