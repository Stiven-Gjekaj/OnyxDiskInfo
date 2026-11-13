//! The warning that each experimental path prints before it runs.
//!
//! An experimental path is code that no drive has answered yet, because no
//! drive that it is for was at hand. It runs only when the person asks for
//! it by name.

/// The lines that a person sees before an experimental path runs.
pub fn warning(what: &str) -> Vec<String> {
    vec![
        "DO NOT TOUCH IF NOT SURE.".to_string(),
        format!("{what} is experimental. No drive has answered it yet."),
        "It can give wrong values, or fail in a way that nobody has seen.".to_string(),
        "Report what it does: https://github.com/Stiven-Gjekaj/OnyxDiskInfo/issues".to_string(),
    ]
}

/// Print the warning to the error stream, so that `--json` stays clean.
pub fn warn(what: &str) {
    for line in warning(what) {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_warning_starts_with_the_disclaimer_and_names_the_path() {
        let lines = warning("ATA SMART on macOS");
        assert_eq!(lines[0], "DO NOT TOUCH IF NOT SURE.");
        assert!(lines[1].starts_with("ATA SMART on macOS is experimental."));
    }
}
