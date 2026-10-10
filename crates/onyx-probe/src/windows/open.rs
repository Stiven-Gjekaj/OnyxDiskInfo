//! The Windows shim that opens a drive for a test.
//!
//! This file makes system calls and decides nothing. Which volumes sit on
//! the drive comes from [`super::volumes::volumes_on_disk`].
//!
//! A test that writes locks and dismounts each volume of the drive first.
//! Windows refuses a write to a sector that a mounted volume holds. The lock
//! comes before the dismount, so the file system cannot mount the volume
//! again during the test. This comes from Burnout.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW,
    FindVolumeClose, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use onyx_core::{DriveInfo, Error, Result};

use super::volumes::{RawVolume, volume_path_to_open, volumes_on_disk};
use crate::open::{Access, refused};

const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;

/// `FILE_SHARE_READ | FILE_SHARE_WRITE`, as `OpenOptions` takes it.
const SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

/// `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`: skip the cache of the
/// host for reads and for writes.
const NO_CACHE: u32 = 0x2000_0000 | 0x8000_0000;

/// A volume that is locked and dismounted. The lock ends when this goes.
pub struct Lock(HANDLE);

impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    std::ffi::OsString::from_wide(&buffer[..end])
        .to_string_lossy()
        .into_owned()
}

fn open_handle(path: &str, access: u32) -> std::io::Result<HANDLE> {
    let path = wide(path);
    // SAFETY: the path is a zero terminated wide string.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            SHARE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}

/// Send one control code with no input, and give back what the driver wrote.
fn control(handle: HANDLE, code: u32, want: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; want];
    let mut written: u32 = 0;
    // SAFETY: the output buffer is owned here and its length is given.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            code,
            std::ptr::null(),
            0,
            out.as_mut_ptr() as *mut c_void,
            out.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    out.truncate(written as usize);
    Some(out)
}

/// Every volume that the system has now, with the disks that each one sits
/// on.
fn volumes() -> Vec<RawVolume> {
    let mut out = Vec::new();
    let mut name = [0u16; 260];
    // SAFETY: the buffer and its length are given together.
    let search = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
    if search == INVALID_HANDLE_VALUE {
        return out;
    }
    loop {
        let text = from_wide(&name);
        let extents = volume_path_to_open(&text)
            .and_then(|path| open_handle(&path, 0).ok())
            .and_then(|handle| {
                let answer = control(handle, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, 8 + 24 * 16);
                // SAFETY: the handle came from CreateFileW and is closed once.
                unsafe { CloseHandle(handle) };
                answer
            });
        out.push(RawVolume {
            guid_path: text,
            extents,
        });
        // SAFETY: the search handle is live and the buffer carries its size.
        let more = unsafe { FindNextVolumeW(search, name.as_mut_ptr(), name.len() as u32) };
        if more == 0 {
            break;
        }
    }
    // SAFETY: the search handle is closed once, here.
    unsafe { FindVolumeClose(search) };
    out
}

/// Lock one volume, then dismount it, and hold the lock.
fn lock_volume(drive: &DriveInfo, path: &str) -> Result<Lock> {
    let handle = open_handle(path, GENERIC_READ | GENERIC_WRITE).map_err(|e| refused(e, path))?;
    let lock = Lock(handle);
    if control(lock.0, FSCTL_LOCK_VOLUME, 0).is_none() {
        // The path names the volume by its GUID, which a person does not
        // know. Explorer shows the volume by its letter.
        return Err(Error::Mounted {
            name: drive.name.clone(),
            by: "a program has a file open on it".to_string(),
        });
    }
    if control(lock.0, FSCTL_DISMOUNT_VOLUME, 0).is_none() {
        return Err(Error::host(
            "FSCTL_DISMOUNT_VOLUME",
            format!("{path} did not dismount"),
        ));
    }
    Ok(lock)
}

/// Open the drive with no cache. A write first locks each volume on it.
pub fn open(drive: &DriveInfo, access: Access) -> Result<(File, Vec<Lock>)> {
    let mut locks = Vec::new();
    if access == Access::Write {
        let number: u32 =
            drive.id.as_str().parse().map_err(|_| {
                Error::host("Windows", format!("{} is not a disk number", drive.id))
            })?;
        for path in volumes_on_disk(&volumes(), number) {
            locks.push(lock_volume(drive, &path)?);
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(access == Access::Write)
        .share_mode(SHARE)
        .custom_flags(NO_CACHE)
        .open(&drive.node)
        .map_err(|e| refused(e, &drive.node))?;
    Ok((file, locks))
}
