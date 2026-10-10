//! The health record of one drive: the evidence, where it came from, and
//! how far to trust it.

use crate::Report;
use crate::health::{Assessment, Confidence, Source};
use crate::measure::Measurement;

/// What Onyx knows about the health of a drive.
// There is one record for each drive, so a box would save nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// The drive reported its health.
    Reported(Report),
    /// The drive reported nothing, and Onyx measured it.
    Measured(Measurement),
}

/// The health record of one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub source: Source,
    pub evidence: Evidence,
}

impl Record {
    /// A record from a report.
    pub fn reported(source: Source, report: Report) -> Self {
        Record {
            source,
            evidence: Evidence::Reported(report),
        }
    }

    /// A record from a measurement. Its source is always
    /// [`Source::Measured`].
    pub fn measured(measurement: Measurement) -> Self {
        Record {
            source: Source::Measured,
            evidence: Evidence::Measured(measurement),
        }
    }

    /// How far a person can trust the verdict.
    pub fn confidence(&self) -> Confidence {
        self.source.confidence()
    }

    /// The verdict and its reasons.
    pub fn assessment(&self) -> Assessment {
        match &self.evidence {
            Evidence::Reported(report) => report.assessment(),
            Evidence::Measured(measurement) => measurement.assessment(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::Verdict;
    use crate::measure::{CHUNK_BYTES, Read};
    use crate::nvme;

    #[test]
    fn a_report_from_the_drive_has_a_high_confidence() {
        let report = Report::nvme(
            &nvme::tests::apple_ssd_identify(),
            &nvme::tests::apple_ssd_log(),
        )
        .unwrap();
        let r = Record::reported(Source::Drive, report);
        assert_eq!(r.confidence(), Confidence::High);
        assert_eq!(r.assessment().verdict, Verdict::Good);
    }

    #[test]
    fn a_measurement_has_a_low_confidence_even_when_it_is_good() {
        let m = Measurement {
            reads: vec![Read::Done {
                offset: 0,
                micros: 30_000,
            }],
            chunk_bytes: CHUNK_BYTES,
        };
        let r = Record::measured(m);
        assert_eq!(r.source, Source::Measured);
        assert_eq!(r.confidence(), Confidence::Low);
        assert_eq!(r.assessment().verdict, Verdict::Good);
    }
}
