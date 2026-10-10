//! The file systems that the tools of the host write: NTFS, ext4, btrfs, XFS,
//! APFS, and HFS+.
//!
//! Each of these has a check tool and a driver of its own host, and a writer
//! that is decades old. Onyx does not write them again. It gives the command
//! line of the tool, and the caller runs it.
//!
//! - Linux: Onyx writes the table, and `mkfs` writes the file system in the
//!   partition.
//! - macOS: `diskutil eraseDisk` writes both.
//! - Windows: PowerShell clears the disk, makes the partition, and formats
//!   it.
//!
//! A host that has no writer for a file system gets an error that names the
//! hosts that have one.

use onyx_core::{Error, Result};

use crate::table::{Kind, Table};

/// A file system that a tool of the host writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostFs {
    Ntfs,
    Ext4,
    Btrfs,
    Xfs,
    Apfs,
    HfsPlus,
}

/// A host, as this module tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Linux,
    Macos,
    Windows,
}

impl Host {
    /// The host that this program runs on, if Onyx knows it.
    pub fn this() -> Option<Host> {
        if cfg!(target_os = "linux") {
            Some(Host::Linux)
        } else if cfg!(target_os = "macos") {
            Some(Host::Macos)
        } else if cfg!(windows) {
            Some(Host::Windows)
        } else {
            None
        }
    }
}

impl HostFs {
    /// The name that a person types and reads.
    pub fn name(self) -> &'static str {
        match self {
            HostFs::Ntfs => "ntfs",
            HostFs::Ext4 => "ext4",
            HostFs::Btrfs => "btrfs",
            HostFs::Xfs => "xfs",
            HostFs::Apfs => "apfs",
            HostFs::HfsPlus => "hfsplus",
        }
    }

    /// The kind that the partition table gives the partition.
    pub fn kind(self) -> Kind {
        match self {
            HostFs::Ntfs => Kind::Ntfs,
            HostFs::Ext4 | HostFs::Btrfs | HostFs::Xfs => Kind::Linux,
            HostFs::Apfs => Kind::Apfs,
            HostFs::HfsPlus => Kind::HfsPlus,
        }
    }

    /// The hosts that have a writer for this file system.
    pub fn hosts(self) -> &'static [Host] {
        match self {
            HostFs::Ntfs => &[Host::Linux, Host::Windows],
            HostFs::Ext4 | HostFs::Btrfs | HostFs::Xfs => &[Host::Linux],
            HostFs::Apfs | HostFs::HfsPlus => &[Host::Macos],
        }
    }
}

/// One program and its arguments. No shell reads them, so a label needs no
/// quotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

impl Command {
    fn new(program: &str, args: &[&str]) -> Self {
        Command {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }
}

fn host_name(host: Host) -> &'static str {
    match host {
        Host::Linux => "Linux",
        Host::Macos => "macOS",
        Host::Windows => "Windows",
    }
}

/// Refuse a file system that this host has no writer for, or that the table
/// cannot hold.
pub fn check(fs: HostFs, table: Table, host: Host) -> Result<()> {
    if !fs.hosts().contains(&host) {
        let names: Vec<&str> = fs.hosts().iter().map(|h| host_name(*h)).collect();
        let verb = if names.len() == 1 { "has" } else { "have" };
        return Err(Error::host(
            "format",
            format!(
                "{} has no tool that writes {}. {} {verb} one.",
                host_name(host),
                fs.name(),
                names.join(" and ")
            ),
        ));
    }
    if table == Table::Mbr && fs.kind().mbr_type().is_none() {
        return Err(Error::host(
            "format",
            format!("{} needs a GPT. Give --table gpt.", fs.name()),
        ));
    }
    Ok(())
}

/// The node of the first partition of a Linux disk. A name that ends in a
/// digit takes a `p` first: `nvme0n1p1` and `loop0p1`, but `sdb1`.
pub fn linux_partition(disk: &str) -> String {
    if disk.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{disk}p1")
    } else {
        format!("{disk}1")
    }
}

/// The `mkfs` that writes `fs` in a Linux partition.
pub fn linux_mkfs(fs: HostFs, partition: &str, label: &str) -> Result<Command> {
    check(fs, Table::Gpt, Host::Linux)?;
    Ok(match fs {
        HostFs::Ntfs => Command::new("mkfs.ntfs", &["--quick", "--label", label, partition]),
        HostFs::Ext4 => Command::new("mkfs.ext4", &["-F", "-L", label, partition]),
        HostFs::Btrfs => Command::new("mkfs.btrfs", &["-f", "-L", label, partition]),
        HostFs::Xfs => Command::new("mkfs.xfs", &["-f", "-L", label, partition]),
        HostFs::Apfs | HostFs::HfsPlus => unreachable!("check refuses them on Linux"),
    })
}

/// The `diskutil eraseDisk` that writes the table and `fs` on a whole macOS
/// disk, such as `disk4`.
pub fn macos_erase(fs: HostFs, table: Table, disk: &str, label: &str) -> Result<Command> {
    check(fs, table, Host::Macos)?;
    let personality = match fs {
        HostFs::Apfs => "APFS",
        HostFs::HfsPlus => "JHFS+",
        _ => unreachable!("check refuses the others on macOS"),
    };
    let scheme = match table {
        Table::Gpt => "GPT",
        Table::Mbr => "MBR",
    };
    Ok(Command::new(
        "diskutil",
        &["eraseDisk", personality, label, scheme, disk],
    ))
}

/// A string as PowerShell reads it between single quotes.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The PowerShell that clears disk `number`, makes one partition, and
/// formats it as `fs`.
pub fn windows_format(fs: HostFs, table: Table, number: u32, label: &str) -> Result<Command> {
    check(fs, table, Host::Windows)?;
    let style = match table {
        Table::Gpt => "GPT",
        Table::Mbr => "MBR",
    };
    // A disk with no table is RAW, and Clear-Disk refuses it. Windows puts
    // the reserved partition of Microsoft on a GPT of its own, so the new
    // partition is the one that New-Partition gives back.
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $n = {number}\n\
         if ((Get-Disk -Number $n).PartitionStyle -ne 'RAW') {{\n\
         \x20   Clear-Disk -Number $n -RemoveData -RemoveOEM -Confirm:$false\n\
         }}\n\
         Initialize-Disk -Number $n -PartitionStyle {style}\n\
         $p = New-Partition -DiskNumber $n -UseMaximumSize\n\
         $p | Format-Volume -FileSystem NTFS -NewFileSystemLabel {label} -Confirm:$false | Out-Null\n",
        label = quoted(label),
    );
    Ok(Command::new(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partition_node_takes_a_p_after_a_digit() {
        assert_eq!(linux_partition("/dev/sdb"), "/dev/sdb1");
        assert_eq!(linux_partition("/dev/nvme0n1"), "/dev/nvme0n1p1");
        assert_eq!(linux_partition("/dev/mmcblk0"), "/dev/mmcblk0p1");
        assert_eq!(linux_partition("/dev/loop3"), "/dev/loop3p1");
    }

    #[test]
    fn linux_runs_mkfs_on_the_partition_with_the_label() {
        let c = linux_mkfs(HostFs::Ext4, "/dev/sdb1", "My Disk").unwrap();
        assert_eq!(c.program, "mkfs.ext4");
        assert_eq!(c.args, ["-F", "-L", "My Disk", "/dev/sdb1"]);
        let c = linux_mkfs(HostFs::Ntfs, "/dev/sdb1", "X").unwrap();
        assert_eq!(c.program, "mkfs.ntfs");
        assert!(c.args.contains(&"--quick".to_string()));
    }

    #[test]
    fn a_host_with_no_writer_names_the_hosts_that_have_one() {
        let e = linux_mkfs(HostFs::Apfs, "/dev/sdb1", "X").unwrap_err();
        assert!(
            e.to_string()
                .contains("Linux has no tool that writes apfs. macOS has one."),
            "{e}"
        );
        let e = check(HostFs::Ntfs, Table::Gpt, Host::Macos).unwrap_err();
        assert!(e.to_string().contains("Linux and Windows have one."), "{e}");
    }

    #[test]
    fn macos_erases_the_whole_disk_with_diskutil() {
        let c = macos_erase(HostFs::HfsPlus, Table::Mbr, "disk4", "Time Machine").unwrap();
        assert_eq!(c.program, "diskutil");
        assert_eq!(
            c.args,
            ["eraseDisk", "JHFS+", "Time Machine", "MBR", "disk4"]
        );
    }

    #[test]
    fn apfs_needs_a_gpt() {
        let e = macos_erase(HostFs::Apfs, Table::Mbr, "disk4", "X").unwrap_err();
        assert!(e.to_string().contains("--table gpt"), "{e}");
        assert!(macos_erase(HostFs::Apfs, Table::Gpt, "disk4", "X").is_ok());
    }

    #[test]
    fn windows_quotes_the_label_and_names_the_disk() {
        let c = windows_format(HostFs::Ntfs, Table::Gpt, 3, "Bob's 'disk'").unwrap();
        let script = c.args.last().unwrap();
        assert!(script.contains("$n = 3\n"), "{script}");
        assert!(script.contains("-PartitionStyle GPT"), "{script}");
        assert!(
            script.contains("-NewFileSystemLabel 'Bob''s ''disk'''"),
            "{script}"
        );
    }
}
