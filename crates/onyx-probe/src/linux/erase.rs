//! The Linux shim for the erase commands of the firmware. Experimental: no
//! drive has taken one from Onyx yet.
//!
//! This file makes system calls and decides nothing. Each decision and each
//! command comes from [`crate::firmware`] and [`super::passthru`], whose
//! tests run on all three hosts.

#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use onyx_core::{Error, Result};

use super::passthru::{
    self, ATA_IDENTIFY, NVME_IDENTIFY_CONTROLLER, NVME_SANITIZE_LOG, SG_DXFER_NONE, SG_DXFER_TO_DEV,
};
use super::smart::{ata_command, nvme_admin, refused, sg_io};
use crate::firmware::{self, Method, SanitizeState};
use crate::sat::{ata_pass_through_16_none, ata_pass_through_16_out};

/// The time that a short security command gets, in milliseconds.
const SHORT_MS: u32 = 30_000;

/// The time between two reads of the Sanitize Status log.
const POLL: Duration = Duration::from_secs(2);

/// The longest time that Onyx waits for a sanitize.
const SANITIZE_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);

/// `BLKRRPART`: `_IO(0x12, 95)`. It tells the kernel to read the partition
/// table again.
const BLKRRPART: u64 = 0x125f;

/// Send the erase to the drive and wait until it ends.
///
/// `step` hears what happens: a short text, and the part of the erase that
/// is done when the drive says it.
pub fn erase(
    file: &File,
    node: &str,
    name: &str,
    method: Method,
    step: &mut dyn FnMut(&str, Option<f64>),
) -> Result<()> {
    let result = if method.is_ata() {
        ata_erase(file, node, name, method, step)
    } else {
        nvme_erase(file, node, name, method, step)
    };
    if result.is_ok() {
        // SAFETY: the file is open, and the ioctl takes no argument. A
        // failure leaves only an old partition table in the kernel.
        unsafe { libc::ioctl(file.as_raw_fd(), BLKRRPART as _) };
    }
    result
}

fn refusal(name: &str, method: Method, reason: String) -> Error {
    Error::EraseRefused {
        name: name.to_string(),
        method: method.name().to_string(),
        reason,
    }
}

/// Send one ATA security command, and fail when the drive refuses it.
fn security(
    file: &File,
    node: &str,
    command: u8,
    block: Option<[u8; 512]>,
    timeout_ms: u32,
) -> Result<()> {
    let status = match block {
        Some(mut block) => sg_io(
            file,
            node,
            &ata_pass_through_16_out(command),
            SG_DXFER_TO_DEV,
            &mut block,
            timeout_ms,
        )?,
        None => sg_io(
            file,
            node,
            &ata_pass_through_16_none(command),
            SG_DXFER_NONE,
            &mut [],
            timeout_ms,
        )?,
    };
    if status.succeeded() {
        Ok(())
    } else {
        Err(Error::host(
            node,
            format!("the drive refused ATA command {command:#04x}: {status:?}"),
        ))
    }
}

fn ata_erase(
    file: &File,
    node: &str,
    name: &str,
    method: Method,
    step: &mut dyn FnMut(&str, Option<f64>),
) -> Result<()> {
    let identify = ata_command(file, node, ATA_IDENTIFY)?.ok_or_else(|| {
        refusal(
            name,
            method,
            "The drive or its USB bridge refused ATA PASS-THROUGH.".to_string(),
        )
    })?;
    let state = firmware::ata_security(&identify);
    if let Some(why) = firmware::ata_refusal(&state, method) {
        return Err(refusal(name, method, why));
    }

    step("Setting the password", None);
    security(
        file,
        node,
        firmware::ATA_SECURITY_SET_PASSWORD,
        Some(firmware::ata_password_block(false)),
        SHORT_MS,
    )?;

    // From here a failure can leave the password on the drive, so each
    // error names it.
    let locked = |e: Error| Error::host(node, format!("{e}. {}", firmware::ata_unlock_hint(node)));
    step("Preparing the erase", None);
    security(
        file,
        node,
        firmware::ATA_SECURITY_ERASE_PREPARE,
        None,
        SHORT_MS,
    )
    .map_err(locked)?;
    step("Erasing. The drive gives no progress", None);
    security(
        file,
        node,
        firmware::ATA_SECURITY_ERASE_UNIT,
        Some(firmware::ata_password_block(
            method == Method::AtaEnhancedSecureErase,
        )),
        firmware::ata_erase_timeout_ms(&state, method),
    )
    .map_err(locked)?;

    let after = ata_command(file, node, ATA_IDENTIFY)?
        .map(|id| firmware::ata_security(&id))
        .unwrap_or_default();
    if after.enabled {
        return Err(locked(Error::host(
            node,
            "the drive ended the erase and kept its password",
        )));
    }
    Ok(())
}

fn nvme_erase(
    file: &File,
    node: &str,
    name: &str,
    method: Method,
    step: &mut dyn FnMut(&str, Option<f64>),
) -> Result<()> {
    let controller = nvme_admin(file, node, NVME_IDENTIFY_CONTROLLER)?;
    let caps = firmware::nvme_erase(&controller);
    if let Some(why) = firmware::nvme_refusal(&caps, method) {
        return Err(refusal(name, method, why));
    }
    match method {
        Method::NvmeFormat | Method::NvmeCryptoFormat => {
            // SAFETY: the file is open, and the ioctl takes no argument.
            let nsid = unsafe { libc::ioctl(file.as_raw_fd(), passthru::NVME_IOCTL_ID as _) };
            if nsid <= 0 {
                return Err(refused(io::Error::last_os_error(), node));
            }
            let nsid = nsid as u32;
            let namespace = nvme_admin(file, node, passthru::nvme_identify_namespace(nsid))?;
            let cdw10 = firmware::format_cdw10(&namespace, method == Method::NvmeCryptoFormat);
            step("Formatting. The drive gives no progress", None);
            nvme_admin(file, node, passthru::nvme_format(nsid, cdw10))?;
            Ok(())
        }
        Method::NvmeBlockSanitize | Method::NvmeCryptoSanitize => {
            let cdw10 = firmware::sanitize_cdw10(method == Method::NvmeCryptoSanitize);
            step("Starting the sanitize", None);
            nvme_admin(file, node, passthru::nvme_sanitize(cdw10))?;
            let start = Instant::now();
            let mut seen_running = false;
            loop {
                std::thread::sleep(POLL);
                let log = nvme_admin(file, node, NVME_SANITIZE_LOG)?;
                match firmware::sanitize_state(&log) {
                    SanitizeState::Running(done) => {
                        seen_running = true;
                        step("Sanitizing", Some(done));
                    }
                    SanitizeState::Done => return Ok(()),
                    SanitizeState::Failed => {
                        return Err(Error::host(node, "the drive says that the sanitize failed"));
                    }
                    SanitizeState::Never if !seen_running && start.elapsed() < POLL * 15 => {}
                    other => {
                        return Err(Error::host(
                            node,
                            format!("the Sanitize Status log gives no known state: {other:?}"),
                        ));
                    }
                }
                if start.elapsed() > SANITIZE_LIMIT {
                    return Err(Error::host(node, "the sanitize did not end in 24 hours"));
                }
            }
        }
        Method::AtaSecureErase | Method::AtaEnhancedSecureErase => Err(refusal(
            name,
            method,
            "An ATA erase does not go to an NVMe drive.".to_string(),
        )),
    }
}
