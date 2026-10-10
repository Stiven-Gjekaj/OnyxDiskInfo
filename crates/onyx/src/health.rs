//! The health command.

use onyx_core::{Error, Result, by_index, in_list_order};

use crate::report;

/// Print the health of the drive with this number.
pub fn run(number: usize, json: bool) -> Result<i32> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    let drive = by_index(&drives, number).ok_or_else(|| Error::NoSuchDrive {
        wanted: number.to_string(),
    })?;
    let report = onyx_probe::read_health(drive)?.decode()?;

    if json {
        println!("{}", report::json(drive, &report));
    } else {
        for line in report::text(drive, &report) {
            println!("{line}");
        }
    }
    Ok(0)
}
