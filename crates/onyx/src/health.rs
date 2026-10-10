//! The health command.

use onyx_core::{Error, Result, by_index, in_list_order};

use crate::report;

/// Print the health of the drive with this number.
pub fn run(number: usize, json: bool) -> Result<i32> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    let drive = by_index(&drives, number).ok_or_else(|| Error::NoSuchDrive {
        wanted: number.to_string(),
    })?;
    let record = onyx_probe::health_record(drive)?;

    if json {
        println!("{}", report::json(drive, &record));
    } else {
        for line in report::text(drive, &record) {
            println!("{line}");
        }
    }
    Ok(0)
}
