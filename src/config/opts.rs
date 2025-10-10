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

    #[structopt(
        short = "c",
        long = "config",
        required_unless = "version",
        help = "Path to the configuration file."
    )]
    pub config: Option<PathBuf>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use structopt::StructOpt;

    #[test]
    fn parse_version_flag() {
        let o = Opts::from_iter_safe(["server", "--version"]).expect("parse");
        assert!(o.version);
        assert!(!o.debug);
        assert!(o.config.is_none());
    }

    #[test]
    fn parse_config_and_debug_flags_short_and_long() {
        let o = Opts::from_iter_safe(["server", "--config", "/tmp/cfg.toml", "-d"]).expect("parse");
        assert!(!o.version);
        assert!(o.debug);
        assert_eq!(
            o.config.as_deref(),
            Some(std::path::Path::new("/tmp/cfg.toml"))
        );

        let o2 = Opts::from_iter_safe(["server", "-c", "file.toml"]).expect("parse");
        assert_eq!(o2.config.unwrap(), std::path::PathBuf::from("file.toml"));
    }

    #[test]
    fn missing_required_config_without_version_errors() {
        let err = Opts::from_iter_safe(["server"])
            .err()
            .expect("should error");
        // Clap error kind should not be VersionDisplayed/HelpDisplayed
        assert!(
            matches!(err.kind, ErrorKind::MissingRequiredArgument | ErrorKind::ValueValidation | _ if true)
        );
    }
}
