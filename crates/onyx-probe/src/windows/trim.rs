//! The Windows shim of a TRIM: `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES`.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::trim::{IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES, windows_trim_request};

/// Trim `length` bytes from `offset`.
pub fn trim(file: &File, offset: u64, length: u64) -> std::io::Result<()> {
    let request = windows_trim_request(offset, length);
    let mut returned: u32 = 0;
    // SAFETY: the input buffer is owned here and its length is given. The
    // call has no output, and it runs to the end before it returns.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES,
            request.as_ptr() as *const c_void,
            request.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
