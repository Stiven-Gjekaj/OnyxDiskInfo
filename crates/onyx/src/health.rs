//! The health command.

use onyx_core::{Error, Result, by_index, in_list_order};

use crate::{experimental, report};

/// Print the health of the drive with this number. `experimental` also
/// tries the paths that no drive has answered yet.
pub fn run(number: usize, json: bool, experimental: bool) -> Result<i32> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    let drive = by_index(&drives, number).ok_or_else(|| Error::NoSuchDrive {
        wanted: number.to_string(),
    })?;
    if experimental {
        experimental::warn("ATA SMART on macOS");
    }
    let record = onyx_probe::health_record_with(drive, experimental)?;

    if json {
        println!("{}", report::json(drive, &record));
    } else {
        for line in report::text(drive, &record) {
            println!("{line}");
        }
    }
    Ok(0)
}
