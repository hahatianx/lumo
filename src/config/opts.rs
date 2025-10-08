use std::path::PathBuf;
use structopt::StructOpt;
use structopt::clap::ErrorKind;

/// Command-line options for the server.
///
/// Examples:
/// - Run with a specific config file:
///   cargo run -- --config config.toml
/// - Show version:
///   cargo run -- --version
///
/// Note: When invoking via `cargo run`, always place `--` before program
/// arguments so Cargo stops parsing its own flags.
#[derive(StructOpt, Debug)]
pub struct Opts {

    #[structopt(short = "v", long = "version")]
    pub version: bool,

    #[structopt(short, long, help = "Enable debug mode (verbose logging)")]
    pub debug: bool,

    #[structopt(short = "c", long = "config", help = "Path to the configuration file. If using `cargo run`, pass after `--`, e.g., `cargo run -- --config config.toml`.")]
    pub config: PathBuf,

}

impl Opts {
    /// Parse CLI arguments. If parsing fails, print the error and the full help, then exit.
    pub fn from_args() -> Self {
        let app = Opts::clap();
        match app.get_matches_safe() {
            Ok(m) => Opts::from_clap(&m),
            Err(e) => {
                let kind = e.kind; // capture before we move/print
                // Print the parsing error (includes a short usage line)
                eprintln!("{}", e);
                // Then print the full help to assist the user
                let mut app = Opts::clap();
                eprintln!();
                let _ = app.print_long_help();
                eprintln!();
                // Exit with the appropriate code
                std::process::exit(match kind {
                    ErrorKind::HelpDisplayed | ErrorKind::VersionDisplayed => 0,
                    _ => 2,
                });
            }
        }
    }
}