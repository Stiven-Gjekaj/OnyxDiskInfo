//! One reading of a drive as a flat list of named numbers.
//!
//! The history keeps readings over months, and it compares each one with the
//! one before. A list of names and numbers is the same shape for NVMe, ATA,
//! eMMC, and a measured drive, so the store and the alert rules need no
//! knowledge of each protocol.
//!
//! The names are part of the file format of the history. Do not change one;
//! add a new one.

use crate::Record;
use crate::Report;
use crate::health::{Confidence, Source, Verdict};
use crate::record::Evidence;

/// One reading of one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub verdict: Verdict,
    pub source: Source,
    pub confidence: Confidence,
    /// Each value and its name, in a fixed order.
    pub values: Vec<(String, i64)>,
}

impl Snapshot {
    /// The value with this name, if the reading has it.
    pub fn get(&self, name: &str) -> Option<i64> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

/// A count of 128 bits in a number that SQLite keeps. No drive counts past
/// `i64::MAX`, and a count that does stays at the top.
fn clamp(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The reading of a record.
pub fn snapshot(record: &Record) -> Snapshot {
    let mut values: Vec<(String, i64)> = Vec::new();
    let mut put = |name: &str, value: i64| values.push((name.to_string(), value));
    match &record.evidence {
        Evidence::Reported(report) => {
            if let Some(t) = report.temperature_celsius() {
                put("temperature", i64::from(t));
            }
            if let Some(h) = report.power_on_hours() {
                put("power_on_hours", clamp(h));
            }
            if let Some(c) = report.power_cycles() {
                put("power_cycles", clamp(c));
            }
            match report {
                Report::Nvme { log, .. } => {
                    put("nvme.critical_warning", i64::from(log.critical_warning));
                    put("nvme.available_spare", i64::from(log.available_spare));
                    put(
                        "nvme.available_spare_threshold",
                        i64::from(log.available_spare_threshold),
                    );
                    put("nvme.percentage_used", i64::from(log.percentage_used));
                    put("nvme.media_errors", clamp(log.media_errors));
                    put("nvme.error_log_entries", clamp(log.error_log_entries));
                    put("nvme.unsafe_shutdowns", clamp(log.unsafe_shutdowns));
                    put("nvme.data_units_read", clamp(log.data_units_read));
                    put("nvme.data_units_written", clamp(log.data_units_written));
                    put(
                        "nvme.warning_temperature_minutes",
                        i64::from(log.warning_temperature_minutes),
                    );
                    put(
                        "nvme.critical_temperature_minutes",
                        i64::from(log.critical_temperature_minutes),
                    );
                }
                Report::Ata { smart, .. } => {
                    for a in &smart.attributes {
                        put(&format!("ata.{}.current", a.id), i64::from(a.current));
                        put(&format!("ata.{}.raw", a.id), clamp(u128::from(a.value())));
                    }
                }
                Report::Emmc { life } => {
                    for (name, value) in [
                        ("emmc.life_a", life.type_a),
                        ("emmc.life_b", life.type_b),
                        ("emmc.pre_eol", life.pre_eol),
                    ] {
                        if let Some(v) = value {
                            put(name, i64::from(v));
                        }
                    }
                }
            }
        }
        Evidence::Measured(m) => {
            put("measured.reads", clamp(m.reads.len() as u128));
            put("measured.failed_reads", clamp(m.errors().count() as u128));
            if let Some(us) = m.median_micros() {
                put("measured.median_micros", clamp(u128::from(us)));
            }
            if let Some(us) = m.max_micros() {
                put("measured.max_micros", clamp(u128::from(us)));
            }
        }
    }
    Snapshot {
        verdict: record.assessment().verdict,
        source: record.source,
        confidence: record.confidence(),
        values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::{Measurement, Read};
    use crate::nvme::tests::{apple_ssd_identify, apple_ssd_log};

    #[test]
    fn an_nvme_reading_names_each_counter() {
        let report = Report::nvme(&apple_ssd_identify(), &apple_ssd_log()).unwrap();
        let s = snapshot(&Record::reported(Source::Drive, report));
        assert_eq!(s.verdict, Verdict::Good);
        assert_eq!(s.confidence, Confidence::High);
        assert!(s.get("temperature").is_some());
        assert!(s.get("power_on_hours").unwrap() > 0);
        assert_eq!(s.get("nvme.media_errors"), Some(0));
        assert!(s.get("nvme.data_units_written").unwrap() > 0);
        assert_eq!(s.get("ata.5.raw"), None);
    }

    #[test]
    fn a_measured_reading_counts_its_failed_reads() {
        let m = Measurement {
            reads: vec![
                Read::Done {
                    offset: 0,
                    micros: 100,
                },
                Read::Failed {
                    offset: 512,
                    message: "gone".into(),
                },
            ],
            chunk_bytes: 512,
        };
        let s = snapshot(&Record::measured(m));
        assert_eq!(s.source, Source::Measured);
        assert_eq!(s.get("measured.reads"), Some(2));
        assert_eq!(s.get("measured.failed_reads"), Some(1));
        assert_eq!(s.get("measured.median_micros"), Some(100));
    }

    #[test]
    fn a_count_past_the_range_of_sqlite_stays_at_the_top() {
        assert_eq!(clamp(u128::MAX), i64::MAX);
        assert_eq!(clamp(7), 7);
    }
}
