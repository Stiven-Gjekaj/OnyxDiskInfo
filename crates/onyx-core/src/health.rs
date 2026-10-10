//! The health of a drive, from what the drive reports.
//!
//! A verdict is one of three words, and it always comes with its reasons. A
//! word with no reason is a word that nobody can check.
//!
//! The rules are deliberately few, and each one names the value that it
//! reads. A drive can fail with a good verdict, and a drive can work for
//! years with a bad one. TERMS.md says so to the person who reads it.

use std::fmt;

use crate::ata::{self, Smart};
use crate::nvme::{HealthLog, critical};

/// The word for the health of a drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// No rule found a problem.
    Good,
    /// A value shows damage or wear. Keep a backup and watch the drive.
    Caution,
    /// The drive says that it fails, or that it can lose data.
    Bad,
}

impl Verdict {
    /// The word that a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Good => "Good",
            Verdict::Caution => "Caution",
            Verdict::Bad => "Bad",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where the health data came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// The drive answered a native command: NVMe, or ATA on a SATA port.
    Drive,
    /// The drive answered through a USB bridge that translated the command.
    Bridge,
    /// `smartctl` read the drive, and Onyx read the JSON of `smartctl`.
    Smartctl,
    /// The drive reports no health. Onyx measured the drive itself.
    Measured,
}

/// How far a person can trust a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Onyx measured the drive. A good result says only that the reads
    /// worked, and not that the medium is healthy.
    Low,
    /// The drive reported the values, and another program decoded them.
    /// Onyx cannot check the bytes.
    Medium,
    /// The drive reported the values, and Onyx decoded the bytes itself.
    High,
}

impl Source {
    /// The word that a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Source::Drive => "the drive",
            Source::Bridge => "the drive, through a USB bridge",
            Source::Smartctl => "smartctl",
            Source::Measured => "measured by Onyx",
        }
    }

    /// The word for a script.
    pub fn key(self) -> &'static str {
        match self {
            Source::Drive => "drive",
            Source::Bridge => "bridge",
            Source::Smartctl => "smartctl",
            Source::Measured => "measured",
        }
    }

    /// The confidence that this source gives.
    ///
    /// A bridge passes the bytes of the drive through unchanged, so Onyx
    /// still decodes them itself.
    pub fn confidence(self) -> Confidence {
        match self {
            Source::Drive | Source::Bridge => Confidence::High,
            Source::Smartctl => Confidence::Medium,
            Source::Measured => Confidence::Low,
        }
    }
}

impl Confidence {
    /// The word that a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

/// A verdict and the reasons for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub verdict: Verdict,
    /// One sentence for each rule that found a problem. Empty when the
    /// verdict is good.
    pub reasons: Vec<String>,
}

impl Assessment {
    fn new() -> Self {
        Assessment {
            verdict: Verdict::Good,
            reasons: Vec::new(),
        }
    }

    /// Add a reason. The verdict is the worst of all the reasons.
    fn add(&mut self, verdict: Verdict, reason: String) {
        self.verdict = self.verdict.max(verdict);
        self.reasons.push(reason);
    }
}

/// The part of its rated life at which an NVMe drive gets a caution.
const NVME_WEAR_CAUTION: u8 = 90;

/// Assess an NVMe drive from its health log.
///
/// - Bad: the drive sets a critical warning bit for its spare, its
///   reliability, read-only media, its backup device, or its persistent
///   memory. Each of these is the drive saying that it can lose data.
/// - Caution: the drive sets the temperature bit, reports a media error, or
///   has used 90 percent or more of its rated life.
pub fn nvme(log: &HealthLog) -> Assessment {
    let mut out = Assessment::new();
    let bits = [
        (
            critical::SPARE,
            "The available spare is below its threshold.",
        ),
        (
            critical::RELIABILITY,
            "The drive says that media or internal errors make it less reliable.",
        ),
        (
            critical::READ_ONLY,
            "The drive put its media in read-only mode.",
        ),
        (
            critical::BACKUP,
            "The volatile memory backup device failed.",
        ),
        (
            critical::PERSISTENT_MEMORY,
            "The persistent memory region is read only.",
        ),
    ];
    for (bit, reason) in bits {
        if log.critical_warning & bit != 0 {
            out.add(Verdict::Bad, reason.to_string());
        }
    }
    if log.critical_warning & critical::TEMPERATURE != 0 {
        out.add(
            Verdict::Caution,
            "A temperature is past a threshold of the drive.".to_string(),
        );
    }
    if log.media_errors > 0 {
        out.add(
            Verdict::Caution,
            format!(
                "The drive reports {} media and data integrity errors.",
                log.media_errors
            ),
        );
    }
    if log.percentage_used >= NVME_WEAR_CAUTION {
        out.add(
            Verdict::Caution,
            format!(
                "The drive has used {} percent of its rated life.",
                log.percentage_used
            ),
        );
    }
    out
}

/// The attributes that count damaged sectors. Any count above zero gets a
/// caution, because each one is a sector that held data and failed.
const SECTOR_COUNTS: [u8; 3] = [
    ata::REALLOCATED_SECTORS,
    ata::PENDING_SECTORS,
    ata::OFFLINE_UNCORRECTABLE,
];

/// Assess an ATA drive from its SMART attributes.
///
/// - Bad: an attribute is at or below its threshold now. This is the test
///   that the drive itself defines.
/// - Caution: an attribute was at or below its threshold in the past, or the
///   drive counts a reallocated, pending, or uncorrectable sector.
pub fn ata(smart: &Smart) -> Assessment {
    let mut out = Assessment::new();
    for a in &smart.attributes {
        let name = ata::attribute_name(a.id).unwrap_or("an attribute with no name");
        if a.failing_now() {
            out.add(
                Verdict::Bad,
                format!(
                    "Attribute {} ({name}) is at {}, and its threshold is {}.",
                    a.id,
                    a.current,
                    a.threshold.unwrap_or(0)
                ),
            );
        } else if a.failed_in_past() {
            out.add(
                Verdict::Caution,
                format!(
                    "Attribute {} ({name}) went down to {} in the past, and its threshold is {}.",
                    a.id,
                    a.worst,
                    a.threshold.unwrap_or(0)
                ),
            );
        }
    }
    for id in SECTOR_COUNTS {
        if let Some(a) = smart.attribute(id).filter(|a| a.raw48() > 0) {
            let name = ata::attribute_name(id).unwrap_or("");
            out.add(
                Verdict::Caution,
                format!("Attribute {id} ({name}) counts {}.", a.raw48()),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nvme::health_log;
    use crate::nvme::tests::apple_ssd_log;

    fn log_with(change: impl FnOnce(&mut Vec<u8>)) -> HealthLog {
        let mut bytes = apple_ssd_log();
        change(&mut bytes);
        health_log(&bytes).unwrap()
    }

    #[test]
    fn the_real_drive_is_good_and_has_no_reasons() {
        let a = nvme(&health_log(&apple_ssd_log()).unwrap());
        assert_eq!(a.verdict, Verdict::Good);
        assert!(a.reasons.is_empty());
    }

    #[test]
    fn each_critical_warning_bit_but_temperature_makes_the_drive_bad() {
        for bit in [
            critical::SPARE,
            critical::RELIABILITY,
            critical::READ_ONLY,
            critical::BACKUP,
            critical::PERSISTENT_MEMORY,
        ] {
            let a = nvme(&log_with(|b| b[0] = bit));
            assert_eq!(a.verdict, Verdict::Bad, "bit {bit:#x}");
            assert_eq!(a.reasons.len(), 1);
        }
    }

    #[test]
    fn the_temperature_bit_is_a_caution() {
        let a = nvme(&log_with(|b| b[0] = critical::TEMPERATURE));
        assert_eq!(a.verdict, Verdict::Caution);
    }

    #[test]
    fn a_media_error_is_a_caution_that_gives_the_count() {
        let a = nvme(&log_with(|b| b[160] = 3));
        assert_eq!(a.verdict, Verdict::Caution);
        assert!(a.reasons[0].contains('3'), "{:?}", a.reasons);
    }

    #[test]
    fn wear_gets_a_caution_at_90_percent_and_not_below() {
        assert_eq!(nvme(&log_with(|b| b[5] = 89)).verdict, Verdict::Good);
        assert_eq!(nvme(&log_with(|b| b[5] = 90)).verdict, Verdict::Caution);
        assert_eq!(nvme(&log_with(|b| b[5] = 140)).verdict, Verdict::Caution);
    }

    #[test]
    fn the_verdict_is_the_worst_reason_and_every_reason_is_kept() {
        let a = nvme(&log_with(|b| {
            b[0] = critical::TEMPERATURE | critical::READ_ONLY;
            b[160] = 1;
        }));
        assert_eq!(a.verdict, Verdict::Bad);
        assert_eq!(a.reasons.len(), 3);
    }

    fn smart_with(change: impl FnOnce(&mut Smart)) -> Smart {
        let (data, limits) = crate::ata::tests::smart_blocks();
        let mut s = crate::ata::smart(&data, Some(&limits)).unwrap();
        change(&mut s);
        s
    }

    #[test]
    fn a_drive_with_no_problem_is_good() {
        let a = ata(&smart_with(|_| {}));
        assert_eq!(a.verdict, Verdict::Good, "{:?}", a.reasons);
    }

    #[test]
    fn an_attribute_at_its_threshold_makes_the_drive_bad() {
        let a = ata(&smart_with(|s| s.attributes[0].current = 10));
        assert_eq!(a.verdict, Verdict::Bad);
        assert!(a.reasons[0].contains("Reallocated"), "{:?}", a.reasons);
    }

    #[test]
    fn an_attribute_that_failed_in_the_past_is_a_caution() {
        let a = ata(&smart_with(|s| s.attributes[0].worst = 5));
        assert_eq!(a.verdict, Verdict::Caution);
    }

    #[test]
    fn each_count_of_damaged_sectors_is_a_caution() {
        for id in SECTOR_COUNTS {
            let a = ata(&smart_with(|s| {
                let slot = s.attributes.iter().position(|a| a.id == id);
                match slot {
                    Some(at) => s.attributes[at].raw = [2, 0, 0, 0, 0, 0],
                    None => {
                        let mut copy = s.attributes[0].clone();
                        copy.id = id;
                        copy.raw = [2, 0, 0, 0, 0, 0];
                        copy.threshold = None;
                        s.attributes.push(copy);
                    }
                }
            }));
            assert_eq!(a.verdict, Verdict::Caution, "attribute {id}");
            assert!(a.reasons.iter().any(|r| r.contains("counts 2")));
        }
    }

    #[test]
    fn a_threshold_of_zero_never_makes_a_drive_bad() {
        // Power-on hours has a threshold of zero. Its current value falls to
        // zero here, and the drive is still good.
        let a = ata(&smart_with(|s| s.attributes[1].current = 0));
        assert_eq!(a.verdict, Verdict::Good, "{:?}", a.reasons);
    }

    #[test]
    fn only_a_measurement_gives_a_low_confidence() {
        assert_eq!(Source::Drive.confidence(), Confidence::High);
        assert_eq!(Source::Bridge.confidence(), Confidence::High);
        assert_eq!(Source::Smartctl.confidence(), Confidence::Medium);
        assert_eq!(Source::Measured.confidence(), Confidence::Low);
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
    }

    #[test]
    fn each_source_has_a_word_for_a_person_and_one_for_a_script() {
        for source in [
            Source::Drive,
            Source::Bridge,
            Source::Smartctl,
            Source::Measured,
        ] {
            assert!(!source.label().is_empty());
            assert!(source.key().chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    #[test]
    fn the_verdicts_are_in_order_of_how_bad_they_are() {
        assert!(Verdict::Good < Verdict::Caution);
        assert!(Verdict::Caution < Verdict::Bad);
    }
}
