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

/// Wait until each write to a drive or an image is on the medium.
pub fn flush(file: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::open::flush(file)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
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

/// Open an image for a disk test: a regular file, or on Linux a block device
/// that no list shows, such as one of the device mapper. Give back the file
/// and its size.
///
/// A block device opens with `O_DIRECT`, and with `O_EXCL` for a write, so
/// the kernel refuses one that a file system or another program holds. That
/// covers a root file system on LVM, which a person could otherwise name
/// here. On macOS and on
/// Windows a block device is refused: the drive list and its checks reach
/// those.
pub fn open_image(path: &std::path::Path, access: Access) -> Result<(File, u64)> {
    use std::io::{Seek, SeekFrom};
    let what = path.display().to_string();
    let metadata = std::fs::metadata(path).map_err(|e| refused(e, &what))?;
    let file_type = metadata.file_type();
    let device = !file_type.is_file();
    if device && !cfg!(target_os = "linux") {
        return Err(onyx_core::Error::host(
            what,
            "this is not a regular file. Give the number of the drive from `onyx list`",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(access == Access::Write);
    // O_DIRECT for a block device: through the page cache, a block reads
    // back from the address that it was written to, and a device that wraps
    // looks genuine.
    #[cfg(target_os = "linux")]
    if device {
        use std::os::unix::fs::OpenOptionsExt;
        let exclusive = if access == Access::Write {
            libc::O_EXCL
        } else {
            0
        };
        options.custom_flags(libc::O_DIRECT | exclusive);
    }
    let mut file = options.open(path).map_err(|e| {
        #[cfg(target_os = "linux")]
        if e.raw_os_error() == Some(libc::EBUSY) {
            return onyx_core::Error::Mounted {
                name: what.clone(),
                by: "a file system or another program holds it".to_string(),
            };
        }
        refused(e, &what)
    })?;
    // A block device reports no length of its own, and a seek to its end
    // gives the size on Linux.
    let size = if device {
        file.seek(SeekFrom::End(0))?
    } else {
        metadata.len()
    };
    Ok((file, size))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_file_opens_with_its_length_as_its_size() {
        let path = std::env::temp_dir().join(format!("onyx-image-{}", std::process::id()));
        std::fs::write(&path, vec![0u8; 8192]).unwrap();
        let (_, size) = open_image(&path, Access::Read).unwrap();
        assert_eq!(size, 8192);
        let (_, size) = open_image(&path, Access::Write).unwrap();
        assert_eq!(size, 8192);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_image_is_an_error() {
        let path = std::env::temp_dir().join("onyx-image-that-is-not-there");
        assert!(open_image(&path, Access::Read).is_err());
    }
}
