//! The command line of Onyx.
//!
//! The binary is one line that calls [`run`]. Everything else is here, so a
//! test reaches each part of it.

use clap::Parser;

pub mod cli;
pub mod format;
pub mod health;
pub mod json;
pub mod list;
pub mod report;
pub mod tests_format;

use cli::{Cli, Command};

/// Run the command that the person typed, and give the exit code.
///
/// An error prints one line on the error stream and gives exit code 1.
pub fn run() -> i32 {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List { json } => list::run(json),
        Command::Health { drive, json } => health::run(drive, json),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("onyx: {e}");
            1
        }
    }
}
