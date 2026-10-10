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

use onyx_core::{DriveInfo, DriveList, Error, Record, Result};

mod health;
pub mod linux;
pub mod macos;
pub mod open;
pub mod sample;
pub mod sat;
pub mod smartctl;
pub mod windows;

pub use health::{RawHealth, source_of};
pub use open::{Access, OpenDrive, open_drive, open_image, open_test_file};

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
        if drive.bus == onyx_core::Bus::Mmc {
            let (life_time, pre_eol_info) =
                linux::parse::emmc_life(&linux::host::RealSysfs, drive.id.as_str());
            if life_time.is_none() && pre_eol_info.is_none() {
                return Err(Error::NoHealthData {
                    name: drive.name.clone(),
                    reason: "the card gives no life estimate".to_string(),
                });
            }
            return Ok(RawHealth::Emmc {
                life_time,
                pre_eol_info,
            });
        }
        linux::smart::ata_health(&drive.node)?.ok_or_else(|| Error::NoHealthData {
            name: drive.name.clone(),
            reason: "the drive refused ATA PASS-THROUGH".to_string(),
        })
    }
    #[cfg(windows)]
    {
        if drive.bus == onyx_core::Bus::Nvme {
            return windows::smart::nvme_health(&drive.node);
        }
        windows::smart::ata_health(&drive.node)?.ok_or_else(|| Error::NoHealthData {
            name: drive.name.clone(),
            reason: "the drive or its driver refused ATA PASS-THROUGH".to_string(),
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = drive;
        Err(Error::Unsupported {
            target: std::env::consts::OS,
        })
    }
}

/// The host that this build runs on, as `smartctl` names its drives.
fn smartctl_host() -> Option<smartctl::Host> {
    if cfg!(target_os = "linux") {
        Some(smartctl::Host::Linux)
    } else if cfg!(target_os = "macos") {
        Some(smartctl::Host::Macos)
    } else if cfg!(windows) {
        Some(smartctl::Host::Windows)
    } else {
        None
    }
}

/// The health record of one drive, from the best source that it has.
///
/// 1. The drive itself, through the commands of Onyx.
/// 2. `smartctl`, if it is on the computer and reads the drive.
/// 3. A timed sample of reads, when the drive reports nothing.
///
/// An error other than "no health data", such as missing rights, stops the
/// chain. A sample with missing rights would only fail the same way.
pub fn health_record(drive: &DriveInfo) -> Result<Record> {
    match read_health(drive) {
        Ok(raw) => return Ok(Record::reported(source_of(drive, &raw), raw.decode()?)),
        Err(Error::NoHealthData { .. }) => {}
        Err(e) => return Err(e),
    }
    if let Some(host) = smartctl_host()
        && let Some(report) = smartctl::run(&smartctl::device_name(host, drive))
    {
        return Ok(Record::reported(
            onyx_core::health::Source::Smartctl,
            report,
        ));
    }
    Ok(Record::measured(sample::sample(drive)?))
}
