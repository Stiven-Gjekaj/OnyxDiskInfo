//! The list command.

use onyx_core::{Result, in_list_order};

use crate::format;

/// Print every drive that this computer reports.
pub fn run(json: bool) -> Result<i32> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);

    if json {
        println!("{}", format::list_json(&drives));
        return Ok(0);
    }

    if drives.is_empty() {
        // Not an error. The computer answered, and the answer is none.
        println!("No drive found.");
        return Ok(0);
    }

    for line in format::table(&drives) {
        println!("{line}");
    }

    if !drives.iter().any(|d| d.system) {
        // Say so rather than stay quiet. A person who cannot see which drive
        // the system starts from needs to know that Onyx cannot either.
        println!();
        println!("Onyx cannot tell which drive this system starts from.");
    }
    Ok(0)
}
