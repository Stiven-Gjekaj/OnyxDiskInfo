//! The Linux shim of a TRIM: `BLKDISCARD`.

#![allow(unsafe_code)]

use std::fs::File;
use std::os::fd::AsRawFd;

/// `BLKDISCARD`: `_IO(0x12, 119)`.
const BLKDISCARD: libc::c_ulong = 0x1277;

/// Discard `length` bytes from `offset`.
pub fn discard(file: &File, offset: u64, length: u64) -> std::io::Result<()> {
    let range: [u64; 2] = [offset, length];
    // SAFETY: the kernel reads two u64 values from the pointer, which points
    // to an array of two that lives until the call returns.
    let answer = unsafe { libc::ioctl(file.as_raw_fd(), BLKDISCARD, range.as_ptr()) };
    if answer < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
