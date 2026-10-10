//! The Linux shim that opens a drive for a test.
//!
//! This file makes system calls and decides nothing. Which mounts hold the
//! drive comes from [`super::parse::mount_points_for_disk`].

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;

use onyx_core::{DriveInfo, Error, Result};

use super::host::RealSysfs;
use super::parse;
use crate::open::{Access, refused};

/// Open the drive. O_DIRECT skips the page cache.
///
/// A write also takes O_EXCL. On a block device, the kernel then refuses the
/// open while a file system or another program holds the drive, so the
/// check below and the open agree, and nothing mounts the drive between them.
pub fn open(drive: &DriveInfo, access: Access) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    let mut flags = libc::O_DIRECT;
    if access == Access::Write {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .map_err(|e| Error::host("/proc/self/mountinfo", e.to_string()))?;
        let points = parse::mount_points_for_disk(&RealSysfs, &mountinfo, drive.id.as_str());
        if let Some(point) = points.first() {
            return Err(Error::Mounted {
                name: drive.name.clone(),
                by: format!("mounted at {point}"),
            });
        }
        options.write(true);
        flags |= libc::O_EXCL;
    }
    options.custom_flags(flags);
    options.open(&drive.node).map_err(|e| {
        if e.raw_os_error() == Some(libc::EBUSY) {
            Error::Mounted {
                name: drive.name.clone(),
                by: "another program holds it".to_string(),
            }
        } else {
            refused(e, &drive.node)
        }
    })
}
