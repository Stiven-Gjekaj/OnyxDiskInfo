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

use onyx_core::{DriveList, Result};

pub mod linux;

/// The drive list of the host that this build runs on.
///
/// A host that Onyx does not support gives an error and not a panic, so the
/// crate still builds and the message still says what is wrong.
pub fn drive_list() -> Result<Box<dyn DriveList>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::host::LinuxDrives))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(onyx_core::Error::Unsupported {
            target: std::env::consts::OS,
        })
    }
}
