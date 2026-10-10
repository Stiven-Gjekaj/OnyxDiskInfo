//! The command line of Onyx.
//!
//! The binary is one line that calls [`run`]. Everything else is here, so a
//! test reaches each part of it.

use clap::Parser;

pub mod cli;
pub mod disk_tests;
pub mod format;
pub mod health;
pub mod json;
pub mod layout;
pub mod list;
pub mod report;
pub mod tests_format;

use cli::{Cli, Command, TestCommand};

/// Run the command that the person typed, and give the exit code.
///
/// An error prints one line on the error stream and gives exit code 1.
pub fn run() -> i32 {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List { json } => list::run(json),
        Command::Health { drive, json } => health::run(drive, json),
        Command::Scan { drive, json } => disk_tests::run_scan(drive, json),
        Command::Bench {
            drive,
            file,
            size_mib,
            keep,
            seconds,
            json,
        } => match (drive, file) {
            (_, Some(path)) => {
                disk_tests::run_bench_file(&path, size_mib.unwrap_or(1024), seconds, keep, json)
            }
            (Some(number), None) => disk_tests::run_bench_drive(number, seconds, json),
            (None, None) => unreachable!("clap requires a drive or a file"),
        },
        Command::Test { test } => match test {
            TestCommand::Capacity { target } => disk_tests::run_capacity(&target),
            TestCommand::Write { target } => disk_tests::run_write(&target),
        },
        Command::Format {
            target,
            fs,
            table,
            label,
        } => layout::run_format(&target, fs, table, &label),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("onyx: {e}");
            1
        }
    }
}
