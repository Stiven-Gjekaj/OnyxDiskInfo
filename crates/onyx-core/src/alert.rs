//! The alerts that two readings of one drive give.
//!
//! An alert says that something changed that a person must know: the
//! verdict got worse, a count of damage went up, the drive says a new
//! warning, or the wear passed a mark. A value that moves all the time, such
//! as the temperature or the data written, makes no alert. The history keeps
//! it for the charts.

use crate::ata;
use crate::health::Verdict;
use crate::snapshot::Snapshot;

/// How much an alert matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Wear that goes as the drive ages. Know it, and plan.
    Notice,
    /// Damage. Keep a backup, and watch the drive.
    Warning,
    /// The drive fails, or says that it can lose data.
    Critical,
}

impl Severity {
    /// The word that a person reads, and that the history keeps.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Notice => "notice",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    /// The severity of a word that [`Severity::label`] gave.
    pub fn from_label(word: &str) -> Option<Severity> {
        match word {
            "notice" => Some(Severity::Notice),
            "warning" => Some(Severity::Warning),
            "critical" => Some(Severity::Critical),
            _ => None,
        }
    }
}

/// One alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub severity: Severity,
    /// The name of the value, as the snapshot gives it, or `verdict`.
    pub value: String,
    pub message: String,
}

/// The counts of damage. Each one only goes up on a drive that gets worse.
const DAMAGE: &[(&str, &str)] = &[
    ("ata.5.raw", "Reallocated sectors"),
    ("ata.196.raw", "Reallocation events"),
    ("ata.197.raw", "Pending sectors"),
    ("ata.198.raw", "Offline uncorrectable sectors"),
    ("ata.187.raw", "Reported uncorrectable errors"),
    ("ata.188.raw", "Command timeouts"),
    ("ata.199.raw", "Interface CRC errors"),
    ("ata.171.raw", "Program fails"),
    ("ata.172.raw", "Erase fails"),
    ("ata.181.raw", "Program fails"),
    ("ata.182.raw", "Erase fails"),
    ("nvme.media_errors", "Media errors"),
    (
        "nvme.warning_temperature_minutes",
        "Minutes above the warning temperature",
    ),
    (
        "nvme.critical_temperature_minutes",
        "Minutes above the critical temperature",
    ),
    ("measured.failed_reads", "Failed reads"),
    ("emmc.pre_eol", "The end-of-life state of the chip"),
];

/// Values that count wear up to 100, and the marks that make an alert.
const WEAR_USED: &[(&str, &str)] = &[("nvme.percentage_used", "The wear of the drive")];

/// Normalized ATA values of wear that go down from 100, by maker.
const WEAR_LEFT: &[u8] = &[177, 202, 231, 232, 233];

/// The marks of wear, in percent used.
const MARKS: &[i64] = &[50, 80, 90, 100];

/// Each alert that `after` gives against `before`, the reading before it.
pub fn compare(before: &Snapshot, after: &Snapshot) -> Vec<Alert> {
    let mut out = Vec::new();
    let mut add = |severity, value: &str, message: String| {
        out.push(Alert {
            severity,
            value: value.to_string(),
            message,
        })
    };

    if after.verdict > before.verdict {
        let severity = if after.verdict == Verdict::Bad {
            Severity::Critical
        } else {
            Severity::Warning
        };
        add(
            severity,
            "verdict",
            format!(
                "The verdict went from {} to {}.",
                before.verdict, after.verdict
            ),
        );
    }

    for (name, words) in DAMAGE {
        if let (Some(a), Some(b)) = (before.get(name), after.get(name))
            && b > a
        {
            add(
                Severity::Warning,
                name,
                format!("{words} went from {a} to {b}."),
            );
        }
    }

    // NVMe critical warning: each bit is a warning of its own.
    if let (Some(a), Some(b)) = (
        before.get("nvme.critical_warning"),
        after.get("nvme.critical_warning"),
    ) {
        let new = b & !a;
        if new != 0 {
            add(
                Severity::Critical,
                "nvme.critical_warning",
                format!("The drive sets a new critical warning: bits {new:#04x}."),
            );
        }
    }

    let passed = |a: i64, b: i64| MARKS.iter().copied().filter(|m| a < *m && b >= *m).max();
    let wear_severity = |mark: i64| {
        if mark >= 90 {
            Severity::Warning
        } else {
            Severity::Notice
        }
    };
    for (name, words) in WEAR_USED {
        if let (Some(a), Some(b)) = (before.get(name), after.get(name))
            && let Some(mark) = passed(a, b)
        {
            add(
                wear_severity(mark),
                name,
                format!("{words} passed {mark} percent: it is {b} percent."),
            );
        }
    }
    for id in WEAR_LEFT {
        let name = format!("ata.{id}.current");
        if let (Some(a), Some(b)) = (before.get(&name), after.get(&name)) {
            // 100 is new, and 0 is worn out: the wear used is 100 less it.
            if let Some(mark) = passed(100 - a, 100 - b) {
                let words = ata::attribute_name(*id).unwrap_or("A value of wear");
                add(
                    wear_severity(mark),
                    &name,
                    format!(
                        "Attribute {id} ({words}) passed {mark} percent of wear: its value is {b}."
                    ),
                );
            }
        }
    }
    for name in ["emmc.life_a", "emmc.life_b"] {
        if let (Some(a), Some(b)) = (before.get(name), after.get(name))
            && b > a
        {
            // The chip counts in steps of 10 percent: 1 is 0 to 10.
            add(
                Severity::Notice,
                name,
                format!(
                    "The chip used up to {} percent of its life, and it was {} percent.",
                    b * 10,
                    a * 10
                ),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{Confidence, Source};

    fn reading(verdict: Verdict, values: &[(&str, i64)]) -> Snapshot {
        Snapshot {
            verdict,
            source: Source::Drive,
            confidence: Confidence::High,
            values: values.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn two_readings_that_are_the_same_give_no_alert() {
        let r = reading(Verdict::Good, &[("ata.5.raw", 3), ("temperature", 40)]);
        assert!(compare(&r, &r).is_empty());
    }

    #[test]
    fn a_count_of_damage_that_goes_up_gives_a_warning() {
        let a = reading(Verdict::Good, &[("ata.5.raw", 0), ("temperature", 40)]);
        let b = reading(Verdict::Good, &[("ata.5.raw", 8), ("temperature", 55)]);
        let alerts = compare(&a, &b);
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].severity, Severity::Warning);
        assert_eq!(alerts[0].value, "ata.5.raw");
        assert_eq!(alerts[0].message, "Reallocated sectors went from 0 to 8.");
    }

    #[test]
    fn a_count_that_goes_down_gives_no_alert() {
        let a = reading(Verdict::Good, &[("ata.197.raw", 4)]);
        let b = reading(Verdict::Good, &[("ata.197.raw", 0)]);
        assert!(compare(&a, &b).is_empty());
    }

    #[test]
    fn a_worse_verdict_gives_an_alert_and_bad_is_critical() {
        let good = reading(Verdict::Good, &[]);
        let caution = reading(Verdict::Caution, &[]);
        let bad = reading(Verdict::Bad, &[]);
        assert_eq!(compare(&good, &caution)[0].severity, Severity::Warning);
        assert_eq!(compare(&caution, &bad)[0].severity, Severity::Critical);
        assert_eq!(
            compare(&good, &bad)[0].message,
            "The verdict went from Good to Bad."
        );
        assert!(compare(&bad, &good).is_empty());
    }

    #[test]
    fn only_a_new_bit_of_the_critical_warning_gives_an_alert() {
        let a = reading(Verdict::Bad, &[("nvme.critical_warning", 0x04)]);
        let b = reading(Verdict::Bad, &[("nvme.critical_warning", 0x06)]);
        let alerts = compare(&a, &b);
        assert_eq!(alerts[0].severity, Severity::Critical);
        assert!(alerts[0].message.contains("0x02"), "{}", alerts[0].message);
        assert!(compare(&b, &b).is_empty());
    }

    #[test]
    fn wear_gives_one_alert_for_the_highest_mark_that_it_passes() {
        let a = reading(Verdict::Good, &[("nvme.percentage_used", 45)]);
        let b = reading(Verdict::Good, &[("nvme.percentage_used", 85)]);
        let alerts = compare(&a, &b);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, Severity::Notice);
        assert!(alerts[0].message.contains("passed 80 percent"));
        let c = reading(Verdict::Good, &[("nvme.percentage_used", 91)]);
        assert_eq!(compare(&b, &c)[0].severity, Severity::Warning);
        assert!(compare(&c, &c).is_empty());
    }

    #[test]
    fn a_normalized_value_of_wear_counts_down_from_100() {
        let a = reading(Verdict::Good, &[("ata.231.current", 52)]);
        let b = reading(Verdict::Good, &[("ata.231.current", 49)]);
        let alerts = compare(&a, &b);
        assert_eq!(alerts.len(), 1);
        assert!(
            alerts[0]
                .message
                .contains("Attribute 231 (Life Left) passed 50")
        );
    }

    #[test]
    fn a_value_that_one_reading_lacks_gives_no_alert() {
        let a = reading(Verdict::Good, &[]);
        let b = reading(Verdict::Good, &[("ata.5.raw", 8)]);
        assert!(compare(&a, &b).is_empty());
    }

    #[test]
    fn a_severity_keeps_its_word() {
        for s in [Severity::Notice, Severity::Warning, Severity::Critical] {
            assert_eq!(Severity::from_label(s.label()), Some(s));
        }
        assert_eq!(Severity::from_label("loud"), None);
    }
}
