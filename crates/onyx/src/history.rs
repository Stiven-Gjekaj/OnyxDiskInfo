//! `onyx watch`, `onyx history`, and `onyx alerts`: the history of each
//! drive, and what changed in it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use onyx_core::alert::Alert;
use onyx_core::snapshot::snapshot;
use onyx_core::{DriveInfo, Error, Result, by_index, in_list_order};
use onyx_store::{Drive, Reading, Store, StoredAlert, Summary};

use crate::cli::StoreArgs;
use crate::json::Json;

/// Open the history that `--db` names, or the one in the data folder.
pub fn open(args: &StoreArgs) -> Result<Store> {
    let path = match &args.db {
        Some(path) => path.clone(),
        None => onyx_store::default_path().ok_or_else(|| {
            Error::host(
                "history",
                "this host gives no folder for the data of an app. Give --db",
            )
        })?,
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::host(dir.display().to_string(), e.to_string()))?;
    }
    Store::open(&path)
}

/// The drive of the history that a drive of the list is.
pub fn drive_of(info: &DriveInfo) -> Drive {
    let model = info.model.as_deref().unwrap_or(&info.name);
    Drive::new(model, info.serial.as_deref(), &info.node)
}

/// The time now, in seconds since 1970.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A time as a person reads it, in UTC: `2026-10-11 12:38 UTC`.
pub fn utc(seconds: i64) -> String {
    // Days since 1970 to a civil date, as Howard Hinnant gives it.
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60
    )
}

fn alert_json(a: &Alert) -> Json {
    Json::Object(vec![
        ("severity", Json::text(a.severity.label())),
        ("value", Json::text(&a.value)),
        ("message", Json::text(&a.message)),
    ])
}

/// What one round of `onyx watch` found for one drive.
pub struct Polled {
    pub info: DriveInfo,
    /// The alerts of the reading, or why there is no reading.
    pub result: std::result::Result<Vec<Alert>, String>,
}

/// Read every drive once, and keep each reading that worked.
fn poll(store: &mut Store, taken_at: i64) -> Result<Vec<Polled>> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    let mut out = Vec::new();
    for info in drives {
        let result = onyx_probe::health_record(&info)
            .map_err(|e| e.to_string())
            .and_then(|record| {
                store
                    .add(&drive_of(&info), taken_at, &snapshot(&record))
                    .map_err(|e| e.to_string())
            });
        out.push(Polled { info, result });
    }
    Ok(out)
}

/// The text of one round of `onyx watch`.
pub fn watch_text(taken_at: i64, polled: &[Polled]) -> Vec<String> {
    let mut out = vec![format!("Reading at {}", utc(taken_at))];
    let mut alerts = 0;
    for p in polled {
        let label = format!("{} ({})", p.info.name, p.info.id);
        match &p.result {
            Ok(list) if list.is_empty() => out.push(format!("  {label}: kept, no change")),
            Ok(list) => {
                for a in list {
                    alerts += 1;
                    out.push(format!("  {label}: {}: {}", a.severity.label(), a.message));
                }
            }
            Err(why) => out.push(format!("  {label}: not read: {why}")),
        }
    }
    out.push(match alerts {
        0 => "No alert.".to_string(),
        1 => "1 alert.".to_string(),
        n => format!("{n} alerts."),
    });
    out
}

/// The JSON of one round of `onyx watch`.
pub fn watch_json(taken_at: i64, polled: &[Polled]) -> Json {
    Json::Object(vec![
        ("taken_at", Json::Signed(taken_at)),
        (
            "drives",
            Json::List(
                polled
                    .iter()
                    .map(|p| {
                        let (stored, alerts, error) = match &p.result {
                            Ok(list) => (
                                true,
                                Json::List(list.iter().map(alert_json).collect()),
                                Json::Null,
                            ),
                            Err(why) => (false, Json::List(Vec::new()), Json::text(why)),
                        };
                        Json::Object(vec![
                            ("id", Json::text(p.info.id.as_str())),
                            ("key", Json::text(drive_of(&p.info).key)),
                            ("stored", Json::Bool(stored)),
                            ("alerts", alerts),
                            ("error", error),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// `onyx watch`: read each drive, keep it, and print what changed. With
/// `once`, the exit code is 2 when a reading gave an alert.
pub fn run_watch(once: bool, interval: u64, args: &StoreArgs) -> Result<i32> {
    let mut store = open(args)?;
    loop {
        let taken_at = now();
        let polled = poll(&mut store, taken_at)?;
        if args.json {
            println!("{}", watch_json(taken_at, &polled));
        } else {
            for line in watch_text(taken_at, &polled) {
                println!("{line}");
            }
        }
        if once {
            let any = polled
                .iter()
                .any(|p| matches!(&p.result, Ok(list) if !list.is_empty()));
            return Ok(if any { 2 } else { 0 });
        }
        std::thread::sleep(Duration::from_secs(interval.max(1)));
    }
}

/// The text of the drives in the history.
pub fn drives_text(summaries: &[Summary]) -> Vec<String> {
    if summaries.is_empty() {
        return vec!["The history is empty. `onyx watch --once` reads each drive.".to_string()];
    }
    summaries
        .iter()
        .map(|s| {
            format!(
                "{}  {}  {} readings, {} to {}, last {}",
                s.drive.model,
                s.drive.serial.as_deref().unwrap_or("-"),
                s.readings,
                utc(s.first_at),
                utc(s.last_at),
                s.last_verdict
            )
        })
        .collect()
}

/// The text of the readings of one drive.
pub fn readings_text(readings: &[Reading]) -> Vec<String> {
    if readings.is_empty() {
        return vec!["The history has no reading of this drive.".to_string()];
    }
    let mut out = vec![format!(
        "{:<20}  {:<8}  {:>11}  {:>10}",
        "TIME", "VERDICT", "TEMPERATURE", "HOURS"
    )];
    for r in readings {
        let t = r
            .snapshot
            .get("temperature")
            .map_or("-".to_string(), |t| format!("{t} C"));
        let h = r
            .snapshot
            .get("power_on_hours")
            .map_or("-".to_string(), |h| h.to_string());
        out.push(format!(
            "{:<20}  {:<8}  {t:>11}  {h:>10}",
            utc(r.taken_at),
            r.snapshot.verdict.label()
        ));
    }
    out
}

/// `onyx history`: the drives in the history, or the readings of one.
pub fn run_history(number: Option<usize>, limit: usize, args: &StoreArgs) -> Result<i32> {
    let store = open(args)?;
    let Some(number) = number else {
        let summaries = store.drives()?;
        if args.json {
            let list = summaries
                .iter()
                .map(|s| {
                    Json::Object(vec![
                        ("key", Json::text(&s.drive.key)),
                        ("model", Json::text(&s.drive.model)),
                        ("serial", Json::maybe_text(s.drive.serial.as_deref())),
                        ("readings", Json::Signed(s.readings)),
                        ("first_at", Json::Signed(s.first_at)),
                        ("last_at", Json::Signed(s.last_at)),
                        ("last_verdict", Json::text(s.last_verdict.label())),
                    ])
                })
                .collect();
            println!("{}", Json::List(list));
        } else {
            for line in drives_text(&summaries) {
                println!("{line}");
            }
        }
        return Ok(0);
    };
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    let info = by_index(&drives, number).ok_or_else(|| Error::NoSuchDrive {
        wanted: number.to_string(),
    })?;
    let readings = store.readings(&drive_of(info).key, limit)?;
    if args.json {
        let list = readings
            .iter()
            .map(|r| {
                Json::Object(vec![
                    ("taken_at", Json::Signed(r.taken_at)),
                    ("verdict", Json::text(r.snapshot.verdict.label())),
                    ("source", Json::text(r.snapshot.source.key())),
                    (
                        "values",
                        Json::List(
                            r.snapshot
                                .values
                                .iter()
                                .map(|(name, value)| {
                                    Json::Object(vec![
                                        ("name", Json::text(name)),
                                        ("value", Json::Signed(*value)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect();
        println!("{}", Json::List(list));
    } else {
        for line in readings_text(&readings) {
            println!("{line}");
        }
    }
    Ok(0)
}

/// The text of the alerts in the history.
pub fn alerts_text(alerts: &[StoredAlert]) -> Vec<String> {
    if alerts.is_empty() {
        return vec!["The history has no alert.".to_string()];
    }
    alerts
        .iter()
        .map(|a| {
            format!(
                "{}  {}  {}: {}",
                utc(a.taken_at),
                a.model,
                a.alert.severity.label(),
                a.alert.message
            )
        })
        .collect()
}

/// `onyx alerts`: the newest alerts of every drive.
pub fn run_alerts(limit: usize, args: &StoreArgs) -> Result<i32> {
    let alerts = open(args)?.alerts(limit)?;
    if args.json {
        let list = alerts
            .iter()
            .map(|a| {
                Json::Object(vec![
                    ("drive", Json::text(&a.drive)),
                    ("model", Json::text(&a.model)),
                    ("taken_at", Json::Signed(a.taken_at)),
                    ("severity", Json::text(a.alert.severity.label())),
                    ("value", Json::text(&a.alert.value)),
                    ("message", Json::text(&a.alert.message)),
                ])
            })
            .collect();
        println!("{}", Json::List(list));
    } else {
        for line in alerts_text(&alerts) {
            println!("{line}");
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::alert::Severity;
    use onyx_core::health::{Confidence, Source, Verdict};
    use onyx_core::snapshot::Snapshot;

    #[test]
    fn a_time_reads_as_a_date_and_a_time_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        // 2026-10-11 12:38:29 UTC.
        assert_eq!(utc(1_791_722_309), "2026-10-11 12:38 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn the_history_of_a_drive_shows_each_reading() {
        let r = Reading {
            taken_at: 0,
            snapshot: Snapshot {
                verdict: Verdict::Caution,
                source: Source::Drive,
                confidence: Confidence::High,
                values: vec![("temperature".into(), 41), ("power_on_hours".into(), 7)],
            },
        };
        let text = readings_text(&[r]);
        assert!(
            text[1].starts_with("1970-01-01 00:00 UTC  Caution"),
            "{}",
            text[1]
        );
        assert!(
            text[1].contains("41 C") && text[1].ends_with('7'),
            "{}",
            text[1]
        );
        assert!(readings_text(&[])[0].contains("no reading"));
    }

    #[test]
    fn a_round_of_watch_says_each_alert_and_each_drive_that_it_could_not_read() {
        let mut a = DriveInfo::new(onyx_core::DriveId::new("sdb"), "/dev/sdb", "sdb");
        a.model = Some("KINGSTON".into());
        let b = DriveInfo::new(onyx_core::DriveId::new("sdc"), "/dev/sdc", "sdc");
        let polled = vec![
            Polled {
                info: a,
                result: Ok(vec![Alert {
                    severity: Severity::Warning,
                    value: "ata.5.raw".into(),
                    message: "Reallocated sectors went from 0 to 3.".into(),
                }]),
            },
            Polled {
                info: b,
                result: Err("needs root".into()),
            },
        ];
        let text = watch_text(0, &polled);
        assert_eq!(
            text[1],
            "  sdb (sdb): warning: Reallocated sectors went from 0 to 3."
        );
        assert_eq!(text[2], "  sdc (sdc): not read: needs root");
        assert_eq!(text[3], "1 alert.");
        let json = watch_json(0, &polled).to_string();
        assert!(json.contains(r#""key":"KINGSTON|/dev/sdb""#), "{json}");
        assert!(json.contains(r#""stored":false"#), "{json}");
    }
}
