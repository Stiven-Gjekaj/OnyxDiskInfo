//! The part of Onyx that asks each host about its drives.
//!
//! Each host is two parts.
//!
//! The shim, `host.rs`, makes system calls and decides nothing. It turns what
//! the host said into plain Rust data and hands it on. It is gated to its own
//! host, so no test reaches it.
//!
//! The parser, `parse.rs`, decides everything and makes no system call. It is
//! compiled on all three hosts, so its tests run three times on every change.
//! Everything that can be wrong lives there.

use onyx_core::{DriveInfo, DriveList, Error, Result};

mod health;
pub mod linux;
pub mod macos;
pub mod windows;

pub use health::RawHealth;

/// The drive list of the host that this build runs on.
///
/// A host that Onyx does not support gives an error and not a panic, so the
/// crate still builds and the message still says what is wrong.
pub fn drive_list() -> Result<Box<dyn DriveList>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::host::LinuxDrives))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::host::MacosDrives))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(windows::host::WindowsDrives))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(onyx_core::Error::Unsupported {
            target: std::env::consts::OS,
        })
    }
}

/// Read the health data of one drive, as raw bytes.
///
/// This sends commands to the drive and reads the answers. It writes
/// nothing. Most hosts need administrator rights for it.
pub fn read_health(drive: &DriveInfo) -> Result<RawHealth> {
    #[cfg(target_os = "macos")]
    {
        macos::smart::nvme_health(drive.id.as_str())?.ok_or_else(|| Error::NoHealthData {
            name: drive.name.clone(),
            reason: "macOS gives no SMART interface for this drive".to_string(),
        })
    }
    #[cfg(target_os = "linux")]
    {
        if drive.bus == onyx_core::Bus::Nvme {
            return linux::smart::nvme_health(&drive.node);
        }
        linux::smart::ata_health(&drive.node)?.ok_or_else(|| Error::NoHealthData {
            name: drive.name.clone(),
            reason: "the drive refused ATA PASS-THROUGH".to_string(),
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = drive;
        Err(Error::Unsupported {
            target: std::env::consts::OS,
        })
    }
}
