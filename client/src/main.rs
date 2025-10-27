mod action;
mod format;

use std::io::{self, Write};

fn main() {
    println!("Local-Disc Client CLI");
    println!("Type 'help' to see available commands. Type 'exit' to quit.\n");

    let stdin = io::stdin();
    let mut line = String::new();

    loop {
        print!("{}", format::prompt());
        // Flush stdout so prompt appears immediately
        let _ = io::stdout().flush();

        line.clear();
        let bytes = stdin.read_line(&mut line);
        match bytes {
            Ok(0) => {
                // EOF (Ctrl-D)
                println!("\nGoodbye.");
                break;
            }
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }

                // Simple command parsing by first token
                let mut parts = input.split_whitespace();
                let cmd = parts.next().unwrap_or("");
                match cmd {
                    "help" => {
                        print!("{}", format::help_text());
                    }
                    "exit" | "quit" => {
                        println!("Goodbye.");
                        break;
                    }
                    "list-peers" => {
                        action::list_peers::list_peers();
                    }
                    _ => {
                        println!(
                            "Unknown command: '{}'. Type 'help' for a list of commands.",
                            input
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }
}
