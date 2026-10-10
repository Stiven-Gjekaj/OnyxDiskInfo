//! The health data of a drive, as the host shim brings it back.
//!
//! The shim sends a command and returns the bytes. It decodes nothing, so
//! the decoders in `onyx-core` and their tests stay the only place that
//! reads a field.

use onyx_core::{Report, Result};

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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
