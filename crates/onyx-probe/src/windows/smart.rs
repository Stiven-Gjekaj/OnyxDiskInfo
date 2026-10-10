//! The Windows shim for health data.
//!
//! This file makes system calls and decides nothing. The buffers come from
//! [`super::passthru`], whose tests run on all three hosts. Nothing here is
//! covered by a test, and nothing here may grow a rule.
//!
//! **This needs Administrator.** Both control codes need a handle with read
//! and write access, although neither one writes to the drive.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use onyx_core::{Error, Result};

use super::passthru::{
    self, ATA_IDENTIFY, ATA_SMART_READ_DATA, ATA_SMART_READ_THRESHOLDS, AtaCommand,
    NVME_HEALTH_LOG, NVME_IDENTIFY_CONTROLLER, NvmeQuery,
};
use crate::health::RawHealth;

/// A device handle that closes itself.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn refused(e: io::Error, what: &str) -> Error {
    if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
        Error::NeedsPrivilege {
            what: what.to_string(),
        }
    } else {
        Error::Io(e)
    }
}

fn open(node: &str) -> Result<Handle> {
    let path: Vec<u16> = std::ffi::OsStr::new(node)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the path is a zero terminated wide string.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(refused(io::Error::last_os_error(), node));
    }
    Ok(Handle(handle))
}

/// Send one control code with one buffer for the question and the answer.
fn control(handle: &Handle, code: u32, buffer: &mut [u8]) -> io::Result<()> {
    let mut written: u32 = 0;
    // SAFETY: the buffer is owned by the caller, and its length is given for
    // the input and for the output alike.
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            code,
            buffer.as_ptr() as *const c_void,
            buffer.len() as u32,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn nvme_query(handle: &Handle, node: &str, query: NvmeQuery) -> Result<Vec<u8>> {
    let mut buffer = passthru::nvme_query_buffer(query);
    control(handle, passthru::IOCTL_STORAGE_QUERY_PROPERTY, &mut buffer)
        .map_err(|e| refused(e, node))?;
    passthru::nvme_answer(&buffer, query.length)
}

/// Read the identify data and the health log of an NVMe drive.
pub fn nvme_health(node: &str) -> Result<RawHealth> {
    let handle = open(node)?;
    Ok(RawHealth::Nvme {
        identify: nvme_query(&handle, node, NVME_IDENTIFY_CONTROLLER)?,
        log: nvme_query(&handle, node, NVME_HEALTH_LOG)?,
    })
}

/// Send one ATA command. `Ok(None)` means the drive or the driver refused it.
fn ata_command(handle: &Handle, node: &str, command: AtaCommand) -> Result<Option<Vec<u8>>> {
    let mut buffer = passthru::ata_buffer(command);
    match control(handle, passthru::IOCTL_ATA_PASS_THROUGH, &mut buffer) {
        Ok(()) => Ok(passthru::ata_answer(&buffer)),
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Err(refused(e, node)),
        // A driver that has no ATA pass-through answers with an error of its
        // own, and that is a refusal and not a failure of Onyx.
        Err(_) => Ok(None),
    }
}

/// Send one ATA command inside ATA PASS-THROUGH, through the SCSI control
/// code. `Ok(None)` means the bridge or the drive refused it.
fn sat_command(handle: &Handle, node: &str, command: AtaCommand) -> Result<Option<Vec<u8>>> {
    let mut buffer = passthru::scsi_buffer(crate::sat::ata_pass_through_16(command));
    match control(handle, passthru::IOCTL_SCSI_PASS_THROUGH, &mut buffer) {
        Ok(()) => Ok(passthru::scsi_answer(&buffer)),
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Err(refused(e, node)),
        Err(_) => Ok(None),
    }
}

/// Read the identify data, the SMART data, and the thresholds of an ATA
/// drive. `Ok(None)` means the drive or its driver refused the commands.
///
/// The ATA control code comes first, because it reaches a drive on a SATA
/// port. A drive behind a USB bridge does not answer it, so SAT through the
/// SCSI control code comes second.
pub fn ata_health(node: &str) -> Result<Option<RawHealth>> {
    let handle = open(node)?;
    type Send = fn(&Handle, &str, AtaCommand) -> Result<Option<Vec<u8>>>;
    let (send, identify): (Send, Vec<u8>) = match ata_command(&handle, node, ATA_IDENTIFY)? {
        Some(identify) => (ata_command, identify),
        None => match sat_command(&handle, node, ATA_IDENTIFY)? {
            Some(identify) => (sat_command, identify),
            None => return Ok(None),
        },
    };
    let Some(smart) = send(&handle, node, ATA_SMART_READ_DATA)? else {
        return Ok(None);
    };
    let thresholds = send(&handle, node, ATA_SMART_READ_THRESHOLDS)?;
    Ok(Some(RawHealth::Ata {
        identify,
        smart,
        thresholds,
    }))
}
