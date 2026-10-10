//! `onyx format`: a new table, one partition, and a file system.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use onyx_core::{DriveInfo, Error, Result, human_size};
use onyx_layout::DriveTarget;
use onyx_layout::fs::FileSystem;
use onyx_layout::host_tool::{self, Host, HostFs};
use onyx_layout::layout::{self, Layout};
use onyx_layout::table::{Extent, Table};

use crate::cli::{FsChoice, TableChoice, WriteTarget};
use crate::disk_tests::{open_for_write, run_mark};
use crate::json::Json;

/// Who writes the file system: Onyx, or a tool of the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Writer {
    Onyx(FileSystem),
    Host(HostFs),
}

fn writer(choice: FsChoice) -> Writer {
    match choice {
        FsChoice::Fat12 => Writer::Onyx(FileSystem::Fat12),
        FsChoice::Fat16 => Writer::Onyx(FileSystem::Fat16),
        FsChoice::Fat32 => Writer::Onyx(FileSystem::Fat32),
        FsChoice::Exfat => Writer::Onyx(FileSystem::Exfat),
        FsChoice::Ntfs => Writer::Host(HostFs::Ntfs),
        FsChoice::Ext4 => Writer::Host(HostFs::Ext4),
        FsChoice::Btrfs => Writer::Host(HostFs::Btrfs),
        FsChoice::Xfs => Writer::Host(HostFs::Xfs),
        FsChoice::Apfs => Writer::Host(HostFs::Apfs),
        FsChoice::Hfsplus => Writer::Host(HostFs::HfsPlus),
    }
}

fn table(choice: TableChoice) -> Table {
    match choice {
        TableChoice::Gpt => Table::Gpt,
        TableChoice::Mbr => Table::Mbr,
    }
}

fn table_name(table: Table) -> &'static str {
    match table {
        Table::Gpt => "gpt",
        Table::Mbr => "mbr",
    }
}

/// What `onyx format --json` prints. A host tool that makes the table itself
/// gives no place of the partition.
pub fn format_json(
    drive: &DriveInfo,
    table: Table,
    fs: &str,
    label: &str,
    part: Option<Extent>,
) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("table", Json::text(table_name(table))),
        ("fs", Json::text(fs)),
        ("label", Json::text(label)),
        (
            "partition_offset",
            Json::maybe_number(part.map(|p| p.start)),
        ),
        (
            "partition_bytes",
            Json::maybe_number(part.map(|p| p.length)),
        ),
    ])
}

/// What `onyx format` prints for a person.
pub fn format_text(
    drive: &DriveInfo,
    table: Table,
    fs: &str,
    label: &str,
    part: Option<Extent>,
) -> String {
    let place = part.map_or(String::new(), |p| {
        format!(" of {} at {}", human_size(p.length), human_size(p.start))
    });
    format!(
        "{} ({}) has a new {} table and one {fs} partition{place}, named {label}.",
        drive.name,
        drive.id,
        table_name(table).to_uppercase(),
    )
}

/// Run a tool, and give its output. A tool that fails gives an error with
/// what it printed.
fn run_tool(command: &host_tool::Command) -> Result<String> {
    let done = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|e| Error::host(command.program.clone(), e.to_string()))?;
    let out = String::from_utf8_lossy(&done.stdout).into_owned();
    if !done.status.success() {
        let err = String::from_utf8_lossy(&done.stderr);
        return Err(Error::host(
            command.program.clone(),
            format!("{}: {}{}", done.status, out.trim(), err.trim()),
        ));
    }
    Ok(out)
}

fn tool(program: &str, args: &[&str]) -> host_tool::Command {
    host_tool::Command {
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
    }
}

/// Wait for a node that the kernel makes after it reads a new table.
fn wait_for(node: &str) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(10);
    while !Path::new(node).exists() {
        if Instant::now() > end {
            return Err(Error::host(
                node.to_string(),
                "the kernel did not make the partition after the new table",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Linux: Onyx writes the table, and mkfs writes the file system in the
/// partition. An image goes on a loop device first.
fn host_linux(
    target: &WriteTarget,
    fs: HostFs,
    table: Table,
    label: &str,
    what: &str,
) -> Result<(DriveInfo, Option<Extent>)> {
    let opened = open_for_write(target, what)?;
    let drive = opened.drive.clone();
    let mut device = DriveTarget::new(
        opened.file.try_clone()?,
        drive.size_bytes,
        drive.logical_sector_size,
    );
    let part = layout::write_table(&mut device, table, fs.kind(), run_mark())?;
    // mkfs opens the partition for itself alone, and the kernel refuses that
    // while Onyx holds the whole drive.
    drop(device);
    drop(opened);
    let disk = match &target.image {
        Some(path) => run_tool(&tool(
            "losetup",
            &[
                "--find",
                "--show",
                "--partscan",
                &path.display().to_string(),
            ],
        ))?
        .trim()
        .to_string(),
        None => {
            run_tool(&tool("blockdev", &["--rereadpt", &drive.node]))?;
            drive.node.clone()
        }
    };
    let partition = host_tool::linux_partition(&disk);
    let result = wait_for(&partition)
        .and_then(|()| host_tool::linux_mkfs(fs, &partition, label))
        .and_then(|mkfs| run_tool(&mkfs));
    if target.image.is_some() {
        run_tool(&tool("losetup", &["--detach", &disk]))?;
    }
    result?;
    Ok((drive, Some(part)))
}

/// macOS: diskutil writes the table and the file system. An image is
/// attached as a disk first.
fn host_macos(
    target: &WriteTarget,
    fs: HostFs,
    table: Table,
    label: &str,
    what: &str,
) -> Result<(DriveInfo, Option<Extent>)> {
    let opened = open_for_write(target, what)?;
    let drive = opened.drive.clone();
    drop(opened);
    let disk = match &target.image {
        Some(path) => {
            let out = run_tool(&tool(
                "hdiutil",
                &[
                    "attach",
                    "-imagekey",
                    "diskimage-class=CRawDiskImage",
                    "-nomount",
                    &path.display().to_string(),
                ],
            ))?;
            let node = out.split_whitespace().next().unwrap_or_default();
            node.trim_start_matches("/dev/").to_string()
        }
        None => drive.id.as_str().to_string(),
    };
    let result = host_tool::macos_erase(fs, table, &disk, label).and_then(|c| run_tool(&c));
    if target.image.is_some() {
        run_tool(&tool("hdiutil", &["detach", "-quiet", &disk]))?;
    }
    result?;
    Ok((drive, None))
}

/// Windows: PowerShell clears the disk, makes the partition, and formats
/// it. Windows formats a disk and not a file, so an image is refused.
fn host_windows(
    target: &WriteTarget,
    fs: HostFs,
    table: Table,
    label: &str,
    what: &str,
) -> Result<(DriveInfo, Option<Extent>)> {
    if target.image.is_some() {
        return Err(Error::host(
            "format",
            "Windows formats NTFS on a drive and not in an image. Attach the image as a VHD and give its number.",
        ));
    }
    let opened = open_for_write(target, what)?;
    let drive = opened.drive.clone();
    // Onyx locks the volumes of the drive while it is open. PowerShell
    // needs them.
    drop(opened);
    let number: u32 = drive
        .id
        .as_str()
        .parse()
        .map_err(|_| Error::host(drive.id.as_str(), "not the number of a disk"))?;
    run_tool(&host_tool::windows_format(fs, table, number, label)?)?;
    Ok((drive, None))
}

/// `onyx format`: write the layout on the drive or the image.
pub fn run_format(
    target: &WriteTarget,
    fs: FsChoice,
    table_choice: TableChoice,
    label: &str,
) -> Result<i32> {
    let table = table(table_choice);
    let (drive, name, part) = match writer(fs) {
        Writer::Onyx(fs) => {
            let layout = Layout {
                table,
                fs,
                label,
                seed: run_mark(),
            };
            let what = format!(
                "The format writes a new {} table and {} and erases every file. It",
                table_name(table).to_uppercase(),
                fs.name(),
            );
            let opened = open_for_write(target, &what)?;
            let mut device = DriveTarget::new(
                opened.file.try_clone()?,
                opened.drive.size_bytes,
                opened.drive.logical_sector_size,
            );
            let part = layout::write(&mut device, &layout)?;
            (opened.drive.clone(), fs.name(), Some(part))
        }
        Writer::Host(fs) => {
            let host = Host::this().ok_or(Error::Unsupported {
                target: std::env::consts::OS,
            })?;
            // Refuse before the prompt, so that a person does not confirm a
            // format that cannot run.
            host_tool::check(fs, table, host)?;
            let what = format!(
                "The format writes a new {} table and {} and erases every file. It",
                table_name(table).to_uppercase(),
                fs.name(),
            );
            let (drive, part) = match host {
                Host::Linux => host_linux(target, fs, table, label, &what)?,
                Host::Macos => host_macos(target, fs, table, label, &what)?,
                Host::Windows => host_windows(target, fs, table, label, &what)?,
            };
            (drive, fs.name(), part)
        }
    };
    if target.json {
        println!("{}", format_json(&drive, table, name, label, part));
    } else {
        println!("{}", format_text(&drive, table, name, label, part));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::DriveId;

    fn image(name: &str, size: u64) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("onyx-format-{}-{name}.img", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(size).unwrap();
        path
    }

    fn target(path: &std::path::Path, confirm: &str) -> WriteTarget {
        WriteTarget {
            drive: None,
            image: Some(path.to_path_buf()),
            force: false,
            confirm: Some(confirm.to_string()),
            json: true,
        }
    }

    #[test]
    fn an_image_gets_the_layout_after_the_phrase() {
        let size = 64 * 1024 * 1024;
        let path = image("fat16", size);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let code = run_format(
            &target(&path, &format!("{name} {size}")),
            FsChoice::Fat16,
            TableChoice::Mbr,
            "CARD",
        )
        .unwrap();
        assert_eq!(code, 0);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        // FAT16 that the host reaches by LBA.
        assert_eq!(bytes[450], 0x0E);
        let boot = &bytes[1024 * 1024..1024 * 1024 + 512];
        assert_eq!(&boot[54..62], b"FAT16   ");
        assert_eq!(&boot[43..54], b"CARD       ");
    }

    #[test]
    fn a_file_system_that_this_host_cannot_write_is_refused_before_the_prompt() {
        let path = image("nohost", 8 * 1024 * 1024);
        // No phrase, and no terminal in a test: a prompt would fail with
        // another error.
        let mut t = target(&path, "");
        t.confirm = None;
        let fs = if cfg!(target_os = "macos") {
            FsChoice::Ext4
        } else {
            FsChoice::Apfs
        };
        let e = run_format(&t, fs, TableChoice::Gpt, "X").unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(e.to_string().contains("has no tool that writes"), "{e}");
    }

    #[test]
    fn a_wrong_phrase_writes_nothing() {
        let path = image("wrong", 8 * 1024 * 1024);
        let e = run_format(
            &target(&path, "another 1"),
            FsChoice::Fat12,
            TableChoice::Mbr,
            "X",
        )
        .unwrap_err();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(e, onyx_core::Error::NotConfirmed), "{e}");
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn the_json_names_the_table_and_the_partition() {
        let mut drive = DriveInfo::new(DriveId::new("disk9"), "/dev/disk9", "disk9");
        drive.size_bytes = 1 << 30;
        let part = Extent {
            start: 1 << 20,
            length: (1 << 30) - (2 << 20),
        };
        assert_eq!(
            format_json(&drive, Table::Gpt, "exfat", "ONYX", Some(part)).to_string(),
            r#"{"id":"disk9","table":"gpt","fs":"exfat","label":"ONYX","partition_offset":1048576,"partition_bytes":1071644672}"#
        );
        assert!(
            format_text(&drive, Table::Gpt, "exfat", "ONYX", Some(part))
                .contains("new GPT table and one exfat partition of")
        );
        assert_eq!(
            format_json(&drive, Table::Gpt, "apfs", "ONYX", None).to_string(),
            r#"{"id":"disk9","table":"gpt","fs":"apfs","label":"ONYX","partition_offset":null,"partition_bytes":null}"#
        );
    }
}
