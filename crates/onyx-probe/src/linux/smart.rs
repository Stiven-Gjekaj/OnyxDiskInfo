//! The Linux shim for health data.
//!
//! This file makes system calls and decides nothing. The commands come from
//! [`super::passthru`], whose tests run on all three hosts. Nothing here is
//! covered by a test, and nothing here may grow a rule.
//!
//! Both ioctls need root: `NVME_IOCTL_ADMIN_CMD` needs `CAP_SYS_ADMIN`, and
//! `SG_IO` with ATA PASS-THROUGH needs `CAP_SYS_RAWIO`.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use onyx_core::{Error, Result};

use super::passthru::{
    self, ATA_IDENTIFY, ATA_SMART_READ_DATA, ATA_SMART_READ_THRESHOLDS, AtaCommand,
    NVME_HEALTH_LOG, NVME_IDENTIFY_CONTROLLER, NvmeCommand, SgStatus,
};
use crate::health::RawHealth;

/// `struct sg_io_hdr` from `<scsi/sg.h>`.
#[repr(C)]
struct SgIoHdr {
    interface_id: i32,
    dxfer_direction: i32,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: *mut c_void,
    cmdp: *const u8,
    sbp: *mut u8,
    timeout: u32,
    flags: u32,
    pack_id: i32,
    usr_ptr: *mut c_void,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32,
    info: u32,
}

/// The time that a drive gets to answer, in milliseconds.
const TIMEOUT_MS: u32 = 10_000;

/// Open a drive to send commands. Nothing here writes to it.
fn open(node: &str) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(node)
        .map_err(|e| refused(e, node))
}

/// The error for a call that the host refused, with missing rights as their
/// own error.
pub(super) fn refused(e: io::Error, what: &str) -> Error {
    match e.raw_os_error() {
        Some(libc::EACCES) | Some(libc::EPERM) => Error::NeedsPrivilege {
            what: what.to_string(),
        },
        _ => Error::Io(e),
    }
}

pub(super) fn nvme_admin(file: &File, node: &str, command: NvmeCommand) -> Result<Vec<u8>> {
    let mut data = vec![0u8; command.data_len as usize];
    let mut bytes = passthru::nvme_command_bytes(command, data.as_mut_ptr() as u64);
    // SAFETY: the command holds the address and the length of `data`, which
    // lives until the call returns. The structure has the size that the
    // ioctl number names.
    let answer = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            passthru::NVME_IOCTL_ADMIN_CMD as _,
            bytes.as_mut_ptr(),
        )
    };
    if answer < 0 {
        return Err(refused(io::Error::last_os_error(), node));
    }
    if answer > 0 {
        return Err(Error::host(
            node,
            format!(
                "the drive refused NVMe opcode {:#04x} with status {answer:#x}",
                command.opcode
            ),
        ));
    }
    Ok(data)
}

/// Read the identify data and the health log of an NVMe drive.
pub fn nvme_health(node: &str) -> Result<RawHealth> {
    let file = open(node)?;
    Ok(RawHealth::Nvme {
        identify: nvme_admin(&file, node, NVME_IDENTIFY_CONTROLLER)?,
        log: nvme_admin(&file, node, NVME_HEALTH_LOG)?,
    })
}

/// Send one ATA command through `SG_IO`. `Ok(None)` means the drive or the
/// bridge refused the command.
pub(super) fn ata_command(file: &File, node: &str, command: AtaCommand) -> Result<Option<Vec<u8>>> {
    let cdb = passthru::ata_pass_through_16(command);
    let mut data = vec![0u8; 512];
    let status = sg_io(
        file,
        node,
        &cdb,
        passthru::SG_DXFER_FROM_DEV,
        &mut data,
        TIMEOUT_MS,
    )?;
    Ok(status.succeeded().then_some(data))
}

/// Send one SCSI command through `SG_IO`, with `data` going the way that
/// `direction` names.
pub(super) fn sg_io(
    file: &File,
    node: &str,
    cdb: &[u8; 16],
    direction: i32,
    data: &mut [u8],
    timeout_ms: u32,
) -> Result<SgStatus> {
    let mut sense = [0u8; 32];
    let mut header = SgIoHdr {
        interface_id: i32::from(b'S'),
        dxfer_direction: direction,
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: data.len() as u32,
        dxferp: data.as_mut_ptr().cast(),
        cmdp: cdb.as_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: timeout_ms,
        flags: 0,
        pack_id: 0,
        usr_ptr: std::ptr::null_mut(),
        status: 0,
        masked_status: 0,
        msg_status: 0,
        sb_len_wr: 0,
        host_status: 0,
        driver_status: 0,
        resid: 0,
        duration: 0,
        info: 0,
    };
    // SAFETY: the header points at the command, the data and the sense
    // buffer, with their lengths, and all three live until the call returns.
    let answer = unsafe { libc::ioctl(file.as_raw_fd(), passthru::SG_IO as _, &mut header) };
    if answer < 0 {
        return Err(refused(io::Error::last_os_error(), node));
    }
    Ok(SgStatus {
        status: header.status,
        host_status: header.host_status,
        driver_status: header.driver_status,
    })
}

/// Read the identify data, the SMART data, and the thresholds of a drive
/// through ATA PASS-THROUGH. `Ok(None)` means the drive refused the
/// commands.
pub fn ata_health(node: &str) -> Result<Option<RawHealth>> {
    let file = open(node)?;
    let Some(identify) = ata_command(&file, node, ATA_IDENTIFY)? else {
        return Ok(None);
    };
    let Some(smart) = ata_command(&file, node, ATA_SMART_READ_DATA)? else {
        return Ok(None);
    };
    let thresholds = ata_command(&file, node, ATA_SMART_READ_THRESHOLDS)?;
    Ok(Some(RawHealth::Ata {
        identify,
        smart,
        thresholds,
    }))
}
