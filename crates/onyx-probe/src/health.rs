//! The health data of a drive, as the host shim brings it back.
//!
//! The shim sends a command and returns the bytes. It decodes nothing, so
//! the decoders in `onyx-core` and their tests stay the only place that
//! reads a field.

use onyx_core::health::Source;
use onyx_core::{Bus, DriveInfo, Report, Result};

/// The raw answers of one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RawHealth {
    /// Identify Controller and the SMART / Health Information log.
    Nvme { identify: Vec<u8>, log: Vec<u8> },
    /// IDENTIFY DEVICE, SMART READ DATA, and SMART READ THRESHOLDS. Some
    /// drives refuse the thresholds.
    Ata {
        identify: Vec<u8>,
        smart: Vec<u8>,
        thresholds: Option<Vec<u8>>,
    },
    /// The two `sysfs` files of an eMMC chip, as text.
    Emmc {
        life_time: Option<String>,
        pre_eol_info: Option<String>,
    },
}

impl RawHealth {
    /// Decode the answers.
    pub fn decode(&self) -> Result<Report> {
        match self {
            RawHealth::Nvme { identify, log } => Report::nvme(identify, log),
            RawHealth::Ata {
                identify,
                smart,
                thresholds,
            } => Report::ata(identify, smart, thresholds.as_deref()),
            RawHealth::Emmc {
                life_time,
                pre_eol_info,
            } => Report::emmc(life_time.as_deref(), pre_eol_info.as_deref()),
        }
    }
}

/// The source of the answers of a drive.
///
/// An ATA answer from a drive on USB came through a bridge that translated
/// the command. Every other answer came from the drive itself.
pub fn source_of(drive: &DriveInfo, raw: &RawHealth) -> Source {
    match raw {
        RawHealth::Ata { .. } if drive.bus == Bus::Usb => Source::Bridge,
        _ => Source::Drive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::DriveId;

    fn drive(bus: Bus) -> DriveInfo {
        let mut d = DriveInfo::new(DriveId::new("sdb"), "/dev/sdb", "a drive");
        d.bus = bus;
        d
    }

    fn ata() -> RawHealth {
        RawHealth::Ata {
            identify: vec![0; 512],
            smart: vec![0; 512],
            thresholds: None,
        }
    }

    #[test]
    fn an_ata_answer_over_usb_came_through_a_bridge() {
        assert_eq!(source_of(&drive(Bus::Usb), &ata()), Source::Bridge);
    }

    #[test]
    fn an_ata_answer_on_sata_came_from_the_drive() {
        assert_eq!(source_of(&drive(Bus::Sata), &ata()), Source::Drive);
    }

    #[test]
    fn an_nvme_answer_came_from_the_drive() {
        let raw = RawHealth::Nvme {
            identify: vec![],
            log: vec![],
        };
        assert_eq!(source_of(&drive(Bus::Nvme), &raw), Source::Drive);
    }

    #[test]
    fn answers_that_are_too_short_give_an_error_and_not_a_panic() {
        let raw = RawHealth::Nvme {
            identify: vec![0; 4],
            log: vec![0; 4],
        };
        assert!(raw.decode().is_err());
        let raw = RawHealth::Ata {
            identify: vec![0; 512],
            smart: vec![0; 512],
            thresholds: None,
        };
        assert!(raw.decode().is_ok());
    }
}
