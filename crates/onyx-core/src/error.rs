//! The error that every operation in Onyx returns.

use std::fmt;

/// What went wrong.
///
/// Each variant names one cause. A message says what the code can prove, and
/// it names the fix when a person can apply one.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The operating system refused a call.
    Io(std::io::Error),
    /// Onyx does not support this operating system.
    Unsupported {
        /// The name of the target, as the compiler gives it.
        target: &'static str,
    },
    /// The host gave an answer that this code cannot read.
    Host {
        /// Where the answer came from, such as a file or a system call.
        source: String,
        /// What is wrong with the answer.
        detail: String,
    },
    /// The drive gives no health data through any way that Onyx knows yet.
    NoHealthData {
        /// The drive, as the list names it.
        name: String,
        /// Why, as far as the code can prove it.
        reason: String,
    },
    /// The host refused the command because the user has too few rights.
    NeedsPrivilege {
        /// What the host refused.
        what: String,
    },
    /// The drive is the one that the running system starts from.
    ///
    /// No option allows a write to it.
    SystemDisk {
        /// The drive, as the list names it.
        name: String,
    },
    /// The host says that the drive refuses each write.
    ReadOnly {
        /// The drive, as the list names it.
        name: String,
    },
    /// The drive is not one that Onyx can prove is removable, and the person
    /// did not pass `--force`.
    NotRemovable {
        /// The drive, as the list names it.
        name: String,
    },
    /// The drive at that number is not the drive that the person saw.
    DriveChanged {
        /// What the person asked for.
        wanted: String,
        /// What is there now.
        found: String,
    },
    /// The person did not type the phrase that confirms the drive.
    NotConfirmed,
    /// A script ran a test that writes, and gave no phrase.
    NeedsConfirmation {
        /// The phrase that `--confirm` takes.
        phrase: String,
    },
    /// A file system holds a part of the drive.
    Mounted {
        /// The drive, as the list names it.
        name: String,
        /// What holds it, such as a mount point.
        by: String,
    },
    /// No drive carries this name or this number.
    NoSuchDrive {
        /// The name or the number that the person gave.
        wanted: String,
    },
}

/// The result of an operation in Onyx.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// An answer from the host that this code cannot read.
    pub fn host(source: impl Into<String>, detail: impl Into<String>) -> Self {
        Error::Host {
            source: source.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Unsupported { target } => {
                write!(f, "Onyx does not support this operating system: {target}")
            }
            Error::Host { source, detail } => write!(f, "{source}: {detail}"),
            Error::NoHealthData { name, reason } => {
                write!(f, "{name} gives no health data: {reason}")
            }
            Error::NeedsPrivilege { what } => write!(
                f,
                "{what} needs administrator rights. Run the command again with sudo, or from an administrator shell"
            ),
            Error::SystemDisk { name } => write!(
                f,
                "{name} is the disk that this system starts from. Onyx never writes to it"
            ),
            Error::ReadOnly { name } => write!(f, "{name} refuses each write"),
            Error::NotRemovable { name } => write!(
                f,
                "{name} is not a removable drive. Add --force if you mean to erase it"
            ),
            Error::DriveChanged { wanted, found } => write!(
                f,
                "the drive changed: you asked for {wanted}, and {found} is there now. Run `onyx list` again"
            ),
            Error::NotConfirmed => write!(f, "the phrase did not match, so nothing was written"),
            Error::NeedsConfirmation { phrase } => write!(
                f,
                "this test writes to the drive, and no terminal can ask for the phrase. Add --confirm \"{phrase}\""
            ),
            Error::Mounted { name, by } => write!(
                f,
                "{name} is in use: {by}. Eject or unmount it, and try again"
            ),
            Error::NoSuchDrive { wanted } => {
                write!(f, "no drive is {wanted}. Run `onyx list` to see the drives")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_error_names_where_the_answer_came_from() {
        let e = Error::host("/sys/block/sda/size", "not a number");
        assert_eq!(e.to_string(), "/sys/block/sda/size: not a number");
    }

    #[test]
    fn an_unknown_drive_tells_the_person_how_to_see_the_drives() {
        let e = Error::NoSuchDrive {
            wanted: "7".to_string(),
        };
        assert!(e.to_string().contains("onyx list"), "{e}");
    }

    #[test]
    fn a_drive_with_no_health_data_says_why() {
        let e = Error::NoHealthData {
            name: "Samsung Flash Drive".to_string(),
            reason: "the USB bridge passes no ATA command".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "Samsung Flash Drive gives no health data: the USB bridge passes no ATA command"
        );
    }

    #[test]
    fn a_refusal_for_rights_names_the_fix() {
        let e = Error::NeedsPrivilege {
            what: "/dev/sda".to_string(),
        };
        assert!(e.to_string().contains("sudo"), "{e}");
    }

    #[test]
    fn a_script_with_no_phrase_learns_the_phrase() {
        let e = Error::NeedsConfirmation {
            phrase: "ProductCode 125829120000".to_string(),
        };
        assert!(
            e.to_string()
                .ends_with("Add --confirm \"ProductCode 125829120000\""),
            "{e}"
        );
    }

    #[test]
    fn an_io_error_keeps_its_cause() {
        let e = Error::from(std::io::Error::other("refused"));
        assert!(std::error::Error::source(&e).is_some());
    }
}
