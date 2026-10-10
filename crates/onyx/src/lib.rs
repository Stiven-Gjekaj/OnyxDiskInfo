//! The command line of Onyx.
//!
//! The binary is one line that calls [`run`]. Everything else is here, so a
//! test reaches each part of it.

use clap::Parser;

pub mod cli;
pub mod format;
pub mod json;

/// Run the command that the person typed, and give the exit code.
pub fn run() -> i32 {
    let _cli = cli::Cli::parse();
    0
}
