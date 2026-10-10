//! The shape of the command line.

use clap::{Parser, Subcommand};

/// Reads the health of every disk, and tests, formats and erases it.
#[derive(Debug, Parser)]
#[command(name = "onyx", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// One thing that Onyx does.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show every drive that this computer reports. This reads and writes
    /// nothing on a drive.
    List {
        /// Print JSON for a script, and not a table for a person.
        #[arg(long)]
        json: bool,
    },
    /// Show the health of one drive. This sends read commands to the drive
    /// and writes nothing. Most hosts need administrator rights for it.
    Health {
        /// The number of the drive, as `onyx list` shows it.
        drive: usize,
        /// Print JSON for a script, and not text for a person.
        #[arg(long)]
        json: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::{CommandFactory, Parser};

    // clap checks the shape of the command only when it runs. This test runs
    // the same checks, so a bad option fails here and not in the hands of a
    // person.
    #[test]
    fn the_command_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_version_flag_is_accepted() {
        let error = Cli::try_parse_from(["onyx", "--version"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn list_takes_a_json_flag() {
        let cli = Cli::try_parse_from(["onyx", "list", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::List { json: true }));
        let cli = Cli::try_parse_from(["onyx", "list"]).unwrap();
        assert!(matches!(cli.command, Command::List { json: false }));
    }

    #[test]
    fn health_takes_the_number_of_a_drive() {
        let cli = Cli::try_parse_from(["onyx", "health", "2", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Health {
                drive: 2,
                json: true
            }
        ));
        assert!(Cli::try_parse_from(["onyx", "health"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "health", "disk0"]).is_err());
    }

    #[test]
    fn a_command_is_required() {
        assert!(Cli::try_parse_from(["onyx"]).is_err());
    }
}
