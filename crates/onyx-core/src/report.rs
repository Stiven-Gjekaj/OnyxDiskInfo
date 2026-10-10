//! The health data of one drive, decoded.
//!
//! A host shim gives raw bytes. This module turns them into one value that
//! the command line and the app read, whatever the protocol of the drive.

use crate::Result;
use crate::ata::{self, Identity, Smart};
use crate::emmc::{self, Life};
use crate::health::{self, Assessment};
use crate::nvme::{self, Controller, HealthLog};

/// What one drive reported about its health.
// There is one report for each drive, so the size of the larger variant
// costs nothing that a box would save.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Report {
    Nvme {
        controller: Controller,
        log: HealthLog,
    },
    Ata {
        identity: Identity,
        smart: Smart,
    },
    Emmc {
        life: Life,
    },
}

impl Report {
    /// Decode the identify data and the health log of an NVMe drive.
    pub fn nvme(identify: &[u8], log: &[u8]) -> Result<Report> {
        Ok(Report::Nvme {
            controller: nvme::controller(identify)?,
            log: nvme::health_log(log)?,
        })
    }

    /// Decode the identify data, the SMART data, and the thresholds of an
    /// ATA drive.
    pub fn ata(identify: &[u8], data: &[u8], thresholds: Option<&[u8]>) -> Result<Report> {
        Ok(Report::Ata {
            identity: ata::identity(identify)?,
            smart: ata::smart(data, thresholds)?,
        })
    }

    /// Decode the two `sysfs` files of an eMMC chip.
    pub fn emmc(life_time: Option<&str>, pre_eol_info: Option<&str>) -> Result<Report> {
        Ok(Report::Emmc {
            life: emmc::from_sysfs(life_time, pre_eol_info)?,
        })
    }

    /// The name of the protocol that carried the data.
    pub fn protocol(&self) -> &'static str {
        match self {
            Report::Nvme { .. } => "NVMe",
            Report::Ata { .. } => "ATA",
            Report::Emmc { .. } => "eMMC",
        }
    }

    /// The verdict and its reasons.
    pub fn assessment(&self) -> Assessment {
        match self {
            Report::Nvme { log, .. } => health::nvme(log),
            Report::Ata { smart, .. } => health::ata(smart),
            Report::Emmc { life } => health::emmc(life),
        }
    }

    /// The temperature in degrees Celsius.
    pub fn temperature_celsius(&self) -> Option<i32> {
        match self {
            Report::Nvme { log, .. } => log.temperature_celsius(),
            Report::Ata { smart, .. } => smart.temperature_celsius(),
            Report::Emmc { .. } => None,
        }
    }

    /// The hours that the drive was on.
    pub fn power_on_hours(&self) -> Option<u128> {
        match self {
            Report::Nvme { log, .. } => Some(log.power_on_hours),
            Report::Ata { smart, .. } => smart
                .attribute(ata::POWER_ON_HOURS)
                .map(|a| u128::from(a.value())),
            Report::Emmc { .. } => None,
        }
    }

    /// The number of times that the drive was switched on.
    pub fn power_cycles(&self) -> Option<u128> {
        match self {
            Report::Nvme { log, .. } => Some(log.power_cycles),
            Report::Ata { smart, .. } => smart
                .attribute(ata::POWER_CYCLES)
                .map(|a| u128::from(a.raw48())),
            Report::Emmc { .. } => None,
        }
    }

    /// The model, the serial number, and the firmware that the drive gave.
    pub fn identity(&self) -> (Option<&str>, Option<&str>, Option<&str>) {
        match self {
            Report::Nvme { controller: c, .. } => (
                c.model.as_deref(),
                c.serial.as_deref(),
                c.firmware.as_deref(),
            ),
            Report::Ata { identity: i, .. } => (
                i.model.as_deref(),
                i.serial.as_deref(),
                i.firmware.as_deref(),
            ),
            // The chip gives its names in `sysfs`, and the drive list holds
            // them already.
            Report::Emmc { .. } => (None, None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::Verdict;

    #[test]
    fn an_nvme_report_reads_the_real_drive() {
        let r = Report::nvme(
            &nvme::tests::apple_ssd_identify(),
            &nvme::tests::apple_ssd_log(),
        )
        .unwrap();
        assert_eq!(r.protocol(), "NVMe");
        assert_eq!(r.temperature_celsius(), Some(35));
        assert_eq!(r.power_on_hours(), Some(111));
        assert_eq!(r.power_cycles(), Some(195));
        assert_eq!(
            r.identity(),
            (
                Some("APPLE SSD AP0512Z"),
                Some("SERIALMASKED0001"),
                Some("241.0.12")
            )
        );
        assert_eq!(r.assessment().verdict, Verdict::Good);
    }

    #[test]
    fn an_ata_report_reads_its_attributes() {
        let (data, limits) = ata::tests::smart_blocks();
        let r = Report::ata(&ata::tests::ssd_identity(), &data, Some(&limits)).unwrap();
        assert_eq!(r.protocol(), "ATA");
        assert_eq!(r.temperature_celsius(), Some(36));
        assert_eq!(r.power_on_hours(), Some(21_544));
        assert_eq!(r.power_cycles(), Some(1_212));
        assert_eq!(r.identity().0, Some("Samsung SSD 860 EVO 1TB"));
    }

    #[test]
    fn an_emmc_report_gives_its_verdict_and_no_counters() {
        let r = Report::emmc(Some("0x0a 0x01"), Some("0x01")).unwrap();
        assert_eq!(r.protocol(), "eMMC");
        assert_eq!(r.assessment().verdict, Verdict::Caution);
        assert_eq!(r.temperature_celsius(), None);
        assert_eq!(r.power_on_hours(), None);
        assert_eq!(r.identity(), (None, None, None));
    }

    #[test]
    fn a_report_from_bytes_that_are_too_short_is_an_error() {
        assert!(Report::nvme(&[0u8; 10], &nvme::tests::apple_ssd_log()).is_err());
        assert!(Report::ata(&[0u8; 512], &[0u8; 10], None).is_err());
    }
}
