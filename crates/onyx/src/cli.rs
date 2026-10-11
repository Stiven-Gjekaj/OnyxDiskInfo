//! The shape of the command line.

use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

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
    /// Read every block of one drive, and time each read. This writes
    /// nothing.
    Scan {
        /// The number of the drive, as `onyx list` shows it.
        drive: usize,
        /// Print JSON for a script, and not text for a person.
        #[arg(long)]
        json: bool,
    },
    /// Measure how fast a drive reads, or how fast a test file reads and
    /// writes. A drive is only read. A test file is written, then deleted.
    #[command(group(ArgGroup::new("target").required(true).args(["drive", "file"])))]
    Bench {
        /// The number of the drive, as `onyx list` shows it.
        drive: Option<usize>,
        /// A new test file on a mounted volume, as CrystalDiskMark uses.
        #[arg(long)]
        file: Option<PathBuf>,
        /// The size of the test file, in MiB. The default is 1024, as in
        /// CrystalDiskMark.
        #[arg(long, conflicts_with = "drive")]
        size_mib: Option<u64>,
        /// Keep the test file at the end, so that another tool can measure
        /// the same file.
        #[arg(long, conflicts_with = "drive")]
        keep: bool,
        /// How long each job runs, in seconds.
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// Print JSON for a script, and not text for a person.
        #[arg(long)]
        json: bool,
    },
    /// A test that writes to a drive.
    Test {
        #[command(subcommand)]
        test: TestCommand,
    },
    /// Write a new partition table with one partition, and a file system in
    /// it. This erases the drive.
    Format {
        #[command(flatten)]
        target: WriteTarget,
        /// The file system.
        #[arg(long, value_enum)]
        fs: FsChoice,
        /// The partition table. MBR is the default, because each camera, car
        /// radio, and console reads it.
        #[arg(long, value_enum, default_value_t = TableChoice::Mbr)]
        table: TableChoice,
        /// The name of the volume. FAT and exFAT keep 11 characters. ext4
        /// keeps 16, and XFS keeps 12.
        #[arg(long, default_value = "ONYX")]
        label: String,
    },
    /// Erase every byte of a drive: write over it, or TRIM it.
    Erase {
        #[command(flatten)]
        target: WriteTarget,
        /// What one pass writes. Give it once for each pass. The default is
        /// one pass of zeros.
        #[arg(long = "pass", value_enum, conflicts_with = "trim")]
        passes: Vec<PassChoice>,
        /// Tell a flash drive that no block holds data, in place of a pass.
        /// It takes seconds. The drive decides when it erases the blocks.
        #[arg(long)]
        trim: bool,
        /// Do not read the last pass back.
        #[arg(long, conflicts_with = "trim")]
        no_check: bool,
    },
}

/// What one pass of `onyx erase` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PassChoice {
    Zero,
    Ones,
    Random,
}

/// A file system that `onyx format` writes. Onyx writes the four FAT and
/// exFAT file systems itself, on each host. A tool of the host writes the
/// others: mkfs on Linux, diskutil on macOS, and Format-Volume on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum FsChoice {
    Fat12,
    Fat16,
    Fat32,
    Exfat,
    /// Linux and Windows.
    Ntfs,
    /// Linux.
    Ext4,
    /// Linux.
    Btrfs,
    /// Linux.
    Xfs,
    /// macOS, on a GPT.
    Apfs,
    /// macOS.
    Hfsplus,
}

/// A partition table that `onyx format` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TableChoice {
    Gpt,
    Mbr,
}

/// A test that writes to a drive. Each one asks for the model and the size of
/// the drive first, and refuses the system disk.
#[derive(Debug, Subcommand)]
pub enum TestCommand {
    /// Find a drive that claims more space than it has. This saves the
    /// blocks that it writes over, and puts them back.
    Capacity {
        #[command(flatten)]
        target: WriteTarget,
    },
    /// Write every block, and read each one back. This erases the drive.
    Write {
        #[command(flatten)]
        target: WriteTarget,
    },
}

/// The drive of a test that writes, and how the person confirms it.
#[derive(Debug, clap::Args)]
#[command(group(ArgGroup::new("target").required(true).args(["drive", "image"])))]
pub struct WriteTarget {
    /// The number of the drive, as `onyx list` shows it.
    pub drive: Option<usize>,
    /// An image file, or on Linux a block device that `onyx list` does not
    /// show, such as one of the device mapper. The kernel refuses one that
    /// is mounted.
    #[arg(long)]
    pub image: Option<PathBuf>,
    /// Allow a drive that Onyx cannot prove is removable. The system disk
    /// stays refused.
    #[arg(long)]
    pub force: bool,
    /// The model and the size of the drive, as the prompt asks for them. For
    /// a script, which cannot answer a prompt.
    #[arg(long)]
    pub confirm: Option<String>,
    /// Print JSON for a script, and not text for a person.
    #[arg(long)]
    pub json: bool,
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
    fn bench_takes_a_drive_or_a_file_and_not_both() {
        assert!(Cli::try_parse_from(["onyx", "bench", "2"]).is_ok());
        assert!(Cli::try_parse_from(["onyx", "bench", "--file", "/tmp/x"]).is_ok());
        assert!(Cli::try_parse_from(["onyx", "bench"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "bench", "2", "--file", "/tmp/x"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "bench", "2", "--size-mib", "8"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "bench", "2", "--keep"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "bench", "--file", "/tmp/x", "--keep"]).is_ok());
    }

    #[test]
    fn a_test_that_writes_takes_a_drive_and_an_optional_phrase() {
        let cli = Cli::try_parse_from([
            "onyx",
            "test",
            "capacity",
            "2",
            "--confirm",
            "ProductCode 125829120000",
        ])
        .unwrap();
        let Command::Test {
            test: super::TestCommand::Capacity { target },
        } = cli.command
        else {
            panic!("not the capacity test");
        };
        assert_eq!(target.drive, Some(2));
        assert!(!target.force);
        assert_eq!(target.confirm.as_deref(), Some("ProductCode 125829120000"));
        assert!(Cli::try_parse_from(["onyx", "test", "write"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "test", "write", "--image", "x.img"]).is_ok());
        assert!(Cli::try_parse_from(["onyx", "test", "write", "2", "--image", "x.img"]).is_err());
    }

    #[test]
    fn format_takes_a_file_system_and_a_table_that_is_mbr_by_default() {
        let cli = Cli::try_parse_from(["onyx", "format", "2", "--fs", "exfat"]).unwrap();
        let Command::Format {
            target,
            fs,
            table,
            label,
        } = cli.command
        else {
            panic!("not format");
        };
        assert_eq!(target.drive, Some(2));
        assert_eq!(fs, super::FsChoice::Exfat);
        assert_eq!(table, super::TableChoice::Mbr);
        assert_eq!(label, "ONYX");
        assert!(Cli::try_parse_from(["onyx", "format", "2"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "format", "2", "--fs", "ext9"]).is_err());
        assert!(
            Cli::try_parse_from([
                "onyx", "format", "--image", "x.img", "--fs", "fat32", "--table", "gpt"
            ])
            .is_ok()
        );
    }

    #[test]
    fn erase_takes_passes_or_a_trim_and_not_both() {
        let cli = Cli::try_parse_from(["onyx", "erase", "2", "--pass", "random", "--pass", "zero"])
            .unwrap();
        let Command::Erase { passes, trim, .. } = cli.command else {
            panic!("not erase");
        };
        assert_eq!(passes, [super::PassChoice::Random, super::PassChoice::Zero]);
        assert!(!trim);
        assert!(Cli::try_parse_from(["onyx", "erase", "2", "--trim"]).is_ok());
        assert!(Cli::try_parse_from(["onyx", "erase", "2", "--trim", "--pass", "zero"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "erase", "2", "--trim", "--no-check"]).is_err());
        assert!(Cli::try_parse_from(["onyx", "erase"]).is_err());
    }

    #[test]
    fn a_command_is_required() {
        assert!(Cli::try_parse_from(["onyx"]).is_err());
    }
}
