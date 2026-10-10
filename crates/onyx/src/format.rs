//! How the list command prints a drive.
//!
//! This module makes no system call and reads no drive. It takes the drives
//! and returns the lines, so a test checks the table on any host.

use onyx_core::{Connection, DriveInfo, human_size};

use crate::json::Json;

/// The word for where a drive sits.
fn place(connection: Connection) -> &'static str {
    match connection {
        Connection::Internal => "internal",
        Connection::External => "external",
        Connection::Unknown => "unknown",
    }
}

/// The whole table, as lines.
pub fn table(drives: &[DriveInfo]) -> Vec<String> {
    const HEADING: [&str; 7] = ["#", "DRIVE", "SIZE", "BUS", "PLACE", "FIRMWARE", "SERIAL"];
    let mut rows: Vec<[String; 7]> = vec![HEADING.map(str::to_string)];

    for (position, drive) in drives.iter().enumerate() {
        rows.push([
            (position + 1).to_string(),
            drive.name.clone(),
            human_size(drive.size_bytes),
            drive.bus.label().to_string(),
            place(drive.connection).to_string(),
            drive.firmware.clone().unwrap_or_else(|| "-".to_string()),
            drive.serial.clone().unwrap_or_else(|| "-".to_string()),
        ]);
    }

    let mut width = [0usize; 7];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            width[column] = width[column].max(cell.chars().count());
        }
    }

    let mut lines = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let mut line = String::new();
        for (column, cell) in row.iter().enumerate() {
            if column > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            // The last column takes no padding, so no line ends in spaces.
            if column + 1 < row.len() {
                for _ in cell.chars().count()..width[column] {
                    line.push(' ');
                }
            }
        }
        // A mark goes after the row, where it reads as a warning and not as
        // a column that somebody has to look up.
        if let Some(drive) = index.checked_sub(1).and_then(|at| drives.get(at)) {
            let marks: Vec<&str> = [
                (drive.system, "system disk"),
                (drive.read_only, "read only"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, mark)| *mark)
            .collect();
            if !marks.is_empty() {
                line.push_str(&format!("   ({})", marks.join(", ")));
            }
        }
        lines.push(line);
    }
    lines
}

/// The list, as `list --json` prints it.
///
/// The object says whether the host named its system disk, because a drive
/// list with no system disk in it is a list that the refusal of that disk did
/// not run against.
pub fn list_json(drives: &[DriveInfo]) -> Json {
    Json::Object(vec![
        (
            "drives",
            Json::List(
                drives
                    .iter()
                    .enumerate()
                    .map(|(at, d)| drive_json(at + 1, d))
                    .collect(),
            ),
        ),
        (
            "system_disk_known",
            Json::Bool(drives.iter().any(|d| d.system)),
        ),
    ])
}

/// One drive, as JSON. `number` is the number that the table prints.
pub fn drive_json(number: usize, drive: &DriveInfo) -> Json {
    Json::Object(vec![
        ("number", Json::Number(number as u64)),
        ("id", Json::text(drive.id.as_str())),
        ("node", Json::text(&drive.node)),
        ("name", Json::text(&drive.name)),
        ("vendor", Json::maybe_text(drive.vendor.as_deref())),
        ("model", Json::maybe_text(drive.model.as_deref())),
        ("serial", Json::maybe_text(drive.serial.as_deref())),
        ("firmware", Json::maybe_text(drive.firmware.as_deref())),
        ("size_bytes", Json::Number(drive.size_bytes)),
        (
            "logical_sector_size",
            Json::Number(drive.logical_sector_size.into()),
        ),
        (
            "physical_sector_size",
            Json::maybe_number(drive.physical_sector_size.map(u64::from)),
        ),
        ("bus", Json::text(drive.bus.label())),
        ("connection", Json::text(place(drive.connection))),
        ("removable", Json::Bool(drive.removable())),
        ("system", Json::Bool(drive.system)),
        ("read_only", Json::Bool(drive.read_only)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::{Bus, Connection, DriveId, DriveInfo};

    fn drive(name: &str, size: u64, bus: Bus, system: bool) -> DriveInfo {
        let mut d = DriveInfo::new(DriveId::new(name), format!("/dev/{name}"), name);
        d.size_bytes = size;
        d.bus = bus;
        d.system = system;
        d.connection = if bus == Bus::Usb {
            Connection::External
        } else {
            Connection::Internal
        };
        d
    }

    #[test]
    fn the_table_numbers_the_drives_from_one() {
        let rows = table(&[
            drive("sda", 1_000_204_886_016, Bus::Nvme, true),
            drive("sdb", 32_010_928_128, Bus::Usb, false),
        ]);
        assert!(rows[1].starts_with('1'));
        assert!(rows[2].starts_with('2'));
    }

    #[test]
    fn the_table_shows_the_firmware_and_the_serial() {
        let mut d = drive("nvme0n1", 2_000_398_934_016, Bus::Nvme, false);
        d.firmware = Some("4B2QJXD7".to_string());
        d.serial = Some("S6B0NL0W123456".to_string());
        let rows = table(&[d]);
        assert!(rows[0].contains("FIRMWARE") && rows[0].contains("SERIAL"));
        assert!(rows[1].contains("4B2QJXD7"), "{}", rows[1]);
        assert!(rows[1].contains("S6B0NL0W123456"), "{}", rows[1]);
        assert!(rows[1].contains("2.0 TB"), "{}", rows[1]);
    }

    #[test]
    fn a_missing_firmware_or_serial_prints_a_dash() {
        let rows = table(&[drive("sdb", 1024, Bus::Usb, false)]);
        let cells: Vec<&str> = rows[1].split_whitespace().collect();
        assert_eq!(cells[cells.len() - 2..], ["-", "-"], "{}", rows[1]);
    }

    #[test]
    fn the_table_says_where_the_drive_sits() {
        let rows = table(&[drive("sdb", 1024, Bus::Usb, false)]);
        assert!(rows[1].contains("external"), "{}", rows[1]);
    }

    #[test]
    fn the_system_disk_is_marked_where_a_person_reads_it() {
        let rows = table(&[drive("sda", 1024, Bus::Nvme, true)]);
        assert!(rows[1].contains("(system disk)"), "{}", rows[1]);
    }

    #[test]
    fn a_read_only_drive_is_marked_where_a_person_reads_it() {
        let mut card = drive("mmcblk0", 1024, Bus::Sd, false);
        card.read_only = true;
        let rows = table(&[card.clone()]);
        assert!(rows[1].ends_with("   (read only)"), "{}", rows[1]);
        card.system = true;
        let rows = table(&[card]);
        assert!(
            rows[1].ends_with("   (system disk, read only)"),
            "{}",
            rows[1]
        );
    }

    #[test]
    fn the_columns_line_up_whatever_the_names_are() {
        let rows = table(&[
            drive("a", 1024, Bus::Usb, false),
            drive("a much longer drive name", 1024, Bus::Usb, false),
        ]);
        let bus_at: Vec<usize> = rows.iter().map(|r| r.find("USB").unwrap_or(0)).collect();
        assert_eq!(bus_at[1], bus_at[2], "the bus column moved between rows");
    }

    #[test]
    fn no_line_ends_in_a_space() {
        let rows = table(&[drive("sdb", 1024, Bus::Usb, false)]);
        for row in &rows {
            assert_eq!(row.trim_end(), row, "{row:?} ends in a space");
        }
    }

    #[test]
    fn the_list_as_json_holds_each_drive_and_whether_the_system_disk_is_known() {
        let mut stick = drive("sdb", 32_010_928_128, Bus::Usb, false);
        stick.model = Some("Ultra \"Fit\"".to_string());
        stick.serial = Some("4C53".to_string());
        stick.firmware = Some("1.00".to_string());
        stick.physical_sector_size = Some(4096);
        let json = list_json(&[drive("sda", 1024, Bus::Nvme, true), stick]).to_string();
        assert_eq!(
            json,
            concat!(
                r#"{"drives":["#,
                r#"{"number":1,"id":"sda","node":"/dev/sda","name":"sda","vendor":null,"model":null,"#,
                r#""serial":null,"firmware":null,"size_bytes":1024,"logical_sector_size":512,"#,
                r#""physical_sector_size":null,"bus":"NVMe","connection":"internal","removable":false,"#,
                r#""system":true,"read_only":false},"#,
                r#"{"number":2,"id":"sdb","node":"/dev/sdb","name":"sdb","vendor":null,"#,
                r#""model":"Ultra \"Fit\"","serial":"4C53","firmware":"1.00","size_bytes":32010928128,"#,
                r#""logical_sector_size":512,"physical_sector_size":4096,"bus":"USB","#,
                r#""connection":"external","removable":true,"system":false,"read_only":false}"#,
                r#"],"system_disk_known":true}"#
            )
        );
    }

    #[test]
    fn a_list_with_no_system_disk_says_so_in_json() {
        let json = list_json(&[drive("sdb", 1024, Bus::Usb, false)]).to_string();
        assert!(json.ends_with(r#""system_disk_known":false}"#), "{json}");
        assert_eq!(
            list_json(&[]).to_string(),
            r#"{"drives":[],"system_disk_known":false}"#
        );
    }

    #[test]
    fn a_table_of_no_drives_still_has_its_heading() {
        let rows = table(&[]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("DRIVE"));
    }
}
