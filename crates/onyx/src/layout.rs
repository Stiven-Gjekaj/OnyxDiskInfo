//! `onyx format`: a new table, one partition, and a file system.

use onyx_core::{DriveInfo, Result, human_size};
use onyx_layout::DriveTarget;
use onyx_layout::fs::FileSystem;
use onyx_layout::layout::{self, Layout};
use onyx_layout::table::{Extent, Table};

use crate::cli::{FsChoice, TableChoice, WriteTarget};
use crate::disk_tests::{open_for_write, run_mark};
use crate::json::Json;

fn file_system(choice: FsChoice) -> FileSystem {
    match choice {
        FsChoice::Fat12 => FileSystem::Fat12,
        FsChoice::Fat16 => FileSystem::Fat16,
        FsChoice::Fat32 => FileSystem::Fat32,
        FsChoice::Exfat => FileSystem::Exfat,
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

/// What `onyx format --json` prints.
pub fn format_json(drive: &DriveInfo, layout: &Layout<'_>, part: Extent) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("table", Json::text(table_name(layout.table))),
        ("fs", Json::text(layout.fs.name())),
        ("label", Json::text(layout.label)),
        ("partition_offset", Json::Number(part.start.into())),
        ("partition_bytes", Json::Number(part.length.into())),
    ])
}

/// What `onyx format` prints for a person.
pub fn format_text(drive: &DriveInfo, layout: &Layout<'_>, part: Extent) -> String {
    format!(
        "{} ({}) has a new {} table and one {} partition of {} at {}, named {}.",
        drive.name,
        drive.id,
        table_name(layout.table).to_uppercase(),
        layout.fs.name(),
        human_size(part.length),
        human_size(part.start),
        layout.label,
    )
}

/// `onyx format`: write the layout on the drive or the image.
pub fn run_format(
    target: &WriteTarget,
    fs: FsChoice,
    table_choice: TableChoice,
    label: &str,
) -> Result<i32> {
    let layout = Layout {
        table: table(table_choice),
        fs: file_system(fs),
        label,
        seed: run_mark(),
    };
    let what = format!(
        "The format writes a new {} table and {} and erases every file. It",
        table_name(layout.table).to_uppercase(),
        layout.fs.name(),
    );
    let opened = open_for_write(target, &what)?;
    let drive = &opened.drive;
    let mut device = DriveTarget::new(
        opened.file.try_clone()?,
        drive.size_bytes,
        drive.logical_sector_size,
    );
    let part = layout::write(&mut device, &layout)?;
    if target.json {
        println!("{}", format_json(drive, &layout, part));
    } else {
        println!("{}", format_text(drive, &layout, part));
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
        let layout = Layout {
            table: Table::Gpt,
            fs: FileSystem::Exfat,
            label: "ONYX",
            seed: 1,
        };
        let part = Extent {
            start: 1 << 20,
            length: (1 << 30) - (2 << 20),
        };
        assert_eq!(
            format_json(&drive, &layout, part).to_string(),
            r#"{"id":"disk9","table":"gpt","fs":"exfat","label":"ONYX","partition_offset":1048576,"partition_bytes":1071644672}"#
        );
        assert!(
            format_text(&drive, &layout, part).contains("new GPT table and one exfat partition")
        );
    }
}
