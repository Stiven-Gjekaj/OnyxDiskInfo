//! The macOS shim of a TRIM: `DKIOCUNMAP`.

#![allow(unsafe_code)]

use std::fs::File;
use std::os::fd::AsRawFd;

/// `dk_extent_t` of `<sys/disk.h>`.
#[repr(C)]
struct Extent {
    offset: u64,
    length: u64,
}

/// `dk_unmap_t` of `<sys/disk.h>`.
#[repr(C)]
struct Unmap {
    extents: *mut Extent,
    count: u32,
    options: u32,
}

/// `DKIOCUNMAP`: `_IOW('d', 31, dk_unmap_t)`. The structure is 16 bytes.
const DKIOCUNMAP: libc::c_ulong = 0x8000_0000 | (16 << 16) | ((b'd' as libc::c_ulong) << 8) | 31;

/// Unmap `length` bytes from `offset`.
pub fn unmap(file: &File, offset: u64, length: u64) -> std::io::Result<()> {
    let mut extent = Extent { offset, length };
    let mut request = Unmap {
        extents: &mut extent,
        count: 1,
        options: 0,
    };
    // SAFETY: the request and the one extent that it points to live until
    // the call returns, and both have the layout of the C structures.
    let answer = unsafe { libc::ioctl(file.as_raw_fd(), DKIOCUNMAP, &mut request) };
    if answer < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
