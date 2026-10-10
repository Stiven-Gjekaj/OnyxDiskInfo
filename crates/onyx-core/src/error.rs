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
    fn an_io_error_keeps_its_cause() {
        let e = Error::from(std::io::Error::other("refused"));
        assert!(std::error::Error::source(&e).is_some());
    }
}
