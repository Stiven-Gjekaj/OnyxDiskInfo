//! Open a drive for a disk test.
//!
//! A test that only reads opens the drive for reading, and nothing else.
//! A test that writes opens it for reading and writing, and first makes sure
//! that no file system holds it: a write under a mounted file system breaks
//! the file system, and the host refuses it on Windows.
//!
//! Each host skips its cache, so a test measures the drive and not the
//! memory of the computer.

use std::fs::File;

use onyx_core::{DriveInfo, Result};

/// What a test does with the drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// A drive that is open for a test.
///
/// On Windows it also holds the locks on the volumes of the drive. The
/// volumes stay dismounted for exactly as long as this value lives.
pub struct OpenDrive {
    pub file: File,
    #[cfg(windows)]
    _locks: Vec<crate::windows::open::Lock>,
}

/// Open one drive for a test.
pub fn open_drive(drive: &DriveInfo, access: Access) -> Result<OpenDrive> {
    #[cfg(target_os = "linux")]
    {
        Ok(OpenDrive {
            file: crate::linux::open::open(drive, access)?,
        })
    }
    #[cfg(target_os = "macos")]
    {
        Ok(OpenDrive {
            file: crate::macos::open::open(drive, access)?,
        })
    }
    #[cfg(windows)]
    {
        let (file, locks) = crate::windows::open::open(drive, access)?;
        Ok(OpenDrive {
            file,
            _locks: locks,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (drive, access);
        Err(onyx_core::Error::Unsupported {
            target: std::env::consts::OS,
        })
    }
}

/// Create a test file for a benchmark, or open the one there, without the
/// cache of the host.
///
/// A benchmark in a file on a mounted volume writes without erasing the
/// drive, as CrystalDiskMark does. The caller deletes the file at the end.
pub fn open_test_file(path: &std::path::Path) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECT);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH.
        options.custom_flags(0x2000_0000 | 0x8000_0000);
    }
    let file = options
        .open(path)
        .map_err(|e| refused(e, &path.display().to_string()))?;
    #[cfg(target_os = "macos")]
    crate::macos::open::no_cache(&file)?;
    Ok(file)
}

/// The error for a call that the host refused, with missing rights as their
/// own error.
pub(crate) fn refused(e: std::io::Error, what: &str) -> onyx_core::Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        onyx_core::Error::NeedsPrivilege {
            what: what.to_string(),
        }
    } else {
        onyx_core::Error::Io(e)
    }
}
