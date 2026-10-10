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
    fn an_io_error_keeps_its_cause() {
        let e = Error::from(std::io::Error::other("refused"));
        assert!(std::error::Error::source(&e).is_some());
    }
}
