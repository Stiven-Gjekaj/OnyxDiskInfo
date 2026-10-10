//! The macOS shim that opens a drive for a test.
//!
//! This file makes system calls and decides nothing. Which mounts hold the
//! drive comes from [`super::parse::holders`].

#![allow(unsafe_code)]

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::MetadataExt;

use onyx_core::{DriveInfo, Error, Result};

use super::{host, parse};
use crate::open::{Access, refused};

/// Each mounted file system: the device number of its source, and its mount
/// point. A source that is not a device, such as a network share, is left
/// out.
fn mounts() -> Vec<(u64, String)> {
    let mut list: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: getmntinfo points `list` at a buffer that it owns and keeps,
    // and gives the number of entries in it.
    let count = unsafe { libc::getmntinfo(&mut list, libc::MNT_NOWAIT) };
    if count <= 0 || list.is_null() {
        return Vec::new();
    }
    // SAFETY: the buffer holds `count` entries, and this code only reads it.
    let entries = unsafe { std::slice::from_raw_parts(list, count as usize) };
    entries
        .iter()
        .filter_map(|e| {
            // SAFETY: both names are zero terminated arrays in the entry.
            let from = unsafe { CStr::from_ptr(e.f_mntfromname.as_ptr()) };
            let on = unsafe { CStr::from_ptr(e.f_mntonname.as_ptr()) };
            let rdev = std::fs::metadata(from.to_str().ok()?).ok()?.rdev();
            Some((rdev, on.to_string_lossy().into_owned()))
        })
        .collect()
}

/// Open the drive through its raw node, which skips the buffer cache.
///
/// A write first refuses a drive that a mounted file system holds. macOS
/// mounts a USB drive the moment it appears, so the message says how to
/// unmount it.
pub fn open(drive: &DriveInfo, access: Access) -> Result<File> {
    if access == Access::Write {
        let snapshot = host::snapshot()?;
        let points = parse::holders(&snapshot, &mounts(), drive.id.as_str());
        if let Some(point) = points.first() {
            return Err(Error::Mounted {
                name: drive.name.clone(),
                by: format!(
                    "mounted at {point}. Run `diskutil unmountDisk {}`",
                    drive.id
                ),
            });
        }
    }
    OpenOptions::new()
        .read(true)
        .write(access == Access::Write)
        .open(&drive.node)
        .map_err(|e| refused(e, &drive.node))
}
