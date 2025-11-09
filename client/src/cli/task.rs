use crate::action;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum TaskCommands {
    List,
}

pub fn handle_task_commands(cmd: &TaskCommands) {
    match cmd {
        TaskCommands::List => action::list_tasks::list_tasks(),
    }
}
