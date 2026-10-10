//! Read one drive with administrator rights, through the command line.
//!
//! The window runs as the person who started it. A drive that needs root
//! gets a button, and the button runs `onyx health <number> --json` through
//! the password dialog of the system: `osascript` on macOS and `pkexec` on
//! Linux. The window reads the JSON that comes back. So the whole app never
//! runs as root, and only the one read does.
//!
//! On Windows the app asks for administrator rights when it starts, as
//! CrystalDiskInfo does, so this module has no work there.

use std::path::Path;
use std::process::Command;

/// The host, as far as the way to ask for rights depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Linux,
    Macos,
}

/// Quote a word for a POSIX shell. Each single quote in it closes the quote,
/// adds an escaped quote, and opens the quote again.
pub fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Quote a text for an AppleScript string. A backslash and a double quote
/// each take a backslash.
pub fn applescript_quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\""))
}

/// The program and the arguments that read one drive with rights.
pub fn command(host: Host, onyx: &Path, number: u64) -> (String, Vec<String>) {
    let onyx = onyx.display().to_string();
    let number = number.to_string();
    match host {
        Host::Linux => (
            "pkexec".to_string(),
            vec![onyx, "health".to_string(), number, "--json".to_string()],
        ),
        Host::Macos => {
            let shell = format!("{} health {number} --json", shell_quote(&onyx));
            let script = format!(
                "do shell script {} with administrator privileges",
                applescript_quote(&shell)
            );
            ("osascript".to_string(), vec!["-e".to_string(), script])
        }
    }
}

/// The host that this build runs on, if it asks for rights this way.
pub fn host() -> Option<Host> {
    if cfg!(target_os = "macos") {
        Some(Host::Macos)
    } else if cfg!(target_os = "linux") {
        Some(Host::Linux)
    } else {
        None
    }
}

/// The command line that sits next to the app.
pub fn onyx_path() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "onyx.exe" } else { "onyx" };
    let path = std::env::current_exe().ok()?.parent()?.join(name);
    path.exists().then_some(path)
}

/// Run the read with rights, and give back the JSON or the reason that it
/// failed. A person who cancels the password dialog gets an error too.
pub fn read(number: u64) -> Result<String, String> {
    let host = host().ok_or("this system asks for rights when the app starts")?;
    let onyx = onyx_path().ok_or("the command line `onyx` is not next to the app")?;
    let (program, args) = command(host, &onyx, number);
    let output = Command::new(&program)
        .args(&args)
        .output()
        .map_err(|e| format!("{program} did not start: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if error.is_empty() {
        "the read with administrator rights did not run".to_string()
    } else {
        error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn linux_runs_onyx_through_pkexec_with_each_argument_apart() {
        let (program, args) = command(Host::Linux, &PathBuf::from("/opt/onyx/onyx"), 2);
        assert_eq!(program, "pkexec");
        assert_eq!(args, ["/opt/onyx/onyx", "health", "2", "--json"]);
    }

    #[test]
    fn macos_runs_onyx_through_osascript_with_the_path_quoted() {
        let (program, args) = command(
            Host::Macos,
            &PathBuf::from("/Applications/OnyxDiskInfo.app/Contents/MacOS/onyx"),
            2,
        );
        assert_eq!(program, "osascript");
        assert_eq!(args[0], "-e");
        assert_eq!(
            args[1],
            r#"do shell script "'/Applications/OnyxDiskInfo.app/Contents/MacOS/onyx' health 2 --json" with administrator privileges"#
        );
    }

    #[test]
    fn a_path_with_quotes_cannot_break_out_of_the_command() {
        // A path is data, and no quote in it may end the string early and
        // run a command of its own.
        let path = PathBuf::from(r#"/tmp/a'b"c\d/onyx"#);
        let (_, args) = command(Host::Macos, &path, 1);
        assert_eq!(
            args[1],
            r#"do shell script "'/tmp/a'\\''b\"c\\d/onyx' health 1 --json" with administrator privileges"#
        );
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(applescript_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
