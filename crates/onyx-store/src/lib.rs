//! The history of each drive: every reading, and every alert that a reading
//! gave, in one SQLite file.
//!
//! A drive is known by its model and its serial number, and not by its
//! number in the list or its node, which change when it moves to another
//! port or another host.

use onyx_core::alert::{self, Alert, Severity};
use onyx_core::health::{Source, Verdict};
use onyx_core::snapshot::Snapshot;
use onyx_core::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};

/// The version of the schema. A new one adds a step to [`migrate`].
const SCHEMA: i64 = 1;

/// The drive that a reading belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// The model and the serial number, or the node when the drive gives no
    /// serial number.
    pub key: String,
    pub model: String,
    pub serial: Option<String>,
}

impl Drive {
    /// A drive with a key made from its model and its serial number. A drive
    /// with no serial number is known by `fallback`, such as its node.
    pub fn new(model: &str, serial: Option<&str>, fallback: &str) -> Self {
        let serial = serial.map(str::trim).filter(|s| !s.is_empty());
        let key = match serial {
            Some(s) => format!("{}|{s}", model.trim()),
            None => format!("{}|{}", model.trim(), fallback),
        };
        Drive {
            key,
            model: model.trim().to_string(),
            serial: serial.map(str::to_string),
        }
    }
}

/// One stored reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// Seconds since 1970, in UTC.
    pub taken_at: i64,
    pub snapshot: Snapshot,
}

/// One stored alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredAlert {
    pub drive: String,
    pub model: String,
    pub taken_at: i64,
    pub alert: Alert,
}

/// What the history holds for one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub drive: Drive,
    pub readings: i64,
    pub first_at: i64,
    pub last_at: i64,
    pub last_verdict: Verdict,
}

/// The history file, open.
pub struct Store {
    db: Connection,
}

fn fail(e: rusqlite::Error) -> Error {
    Error::host("history", e.to_string())
}

fn verdict_from(word: &str) -> Verdict {
    match word {
        "Bad" => Verdict::Bad,
        "Caution" => Verdict::Caution,
        _ => Verdict::Good,
    }
}

fn source_from(word: &str) -> Source {
    match word {
        "drive" => Source::Drive,
        "bridge" => Source::Bridge,
        "smartctl" => Source::Smartctl,
        _ => Source::Measured,
    }
}

/// Make the tables of a new file, or bring an old file up to [`SCHEMA`].
fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        db.execute_batch(
            "BEGIN;
             CREATE TABLE drive (
                 id INTEGER PRIMARY KEY,
                 key TEXT NOT NULL UNIQUE,
                 model TEXT NOT NULL,
                 serial TEXT
             );
             CREATE TABLE reading (
                 id INTEGER PRIMARY KEY,
                 drive INTEGER NOT NULL REFERENCES drive(id),
                 taken_at INTEGER NOT NULL,
                 verdict TEXT NOT NULL,
                 source TEXT NOT NULL
             );
             CREATE INDEX reading_by_drive ON reading(drive, taken_at);
             CREATE TABLE value (
                 reading INTEGER NOT NULL REFERENCES reading(id),
                 position INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 value INTEGER NOT NULL,
                 PRIMARY KEY (reading, name)
             ) WITHOUT ROWID;
             CREATE TABLE alert (
                 id INTEGER PRIMARY KEY,
                 reading INTEGER NOT NULL REFERENCES reading(id),
                 severity TEXT NOT NULL,
                 value TEXT NOT NULL,
                 message TEXT NOT NULL
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    Ok(())
}

impl Store {
    /// Open the history file, and make it when there is none.
    pub fn open(path: &std::path::Path) -> Result<Store> {
        let db = Connection::open(path).map_err(fail)?;
        Store::from(db)
    }

    /// A history in memory, for a test.
    pub fn in_memory() -> Result<Store> {
        Store::from(Connection::open_in_memory().map_err(fail)?)
    }

    fn from(db: Connection) -> Result<Store> {
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(fail)?;
        if version > SCHEMA {
            return Err(Error::host(
                "history",
                format!(
                    "a newer Onyx wrote this file, with schema {version}. This one reads up to {SCHEMA}"
                ),
            ));
        }
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(fail)?;
        migrate(&db).map_err(fail)?;
        Ok(Store { db })
    }

    fn drive_id(&self, key: &str) -> Result<Option<i64>> {
        self.db
            .query_row("SELECT id FROM drive WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(fail)
    }

    fn values(&self, reading: i64) -> Result<Vec<(String, i64)>> {
        let mut q = self
            .db
            .prepare_cached("SELECT name, value FROM value WHERE reading = ?1 ORDER BY position")
            .map_err(fail)?;
        let rows = q
            .query_map([reading], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(fail)?;
        rows.collect::<rusqlite::Result<_>>().map_err(fail)
    }

    fn reading_at(&self, id: i64, taken_at: i64, verdict: &str, source: &str) -> Result<Reading> {
        let source = source_from(source);
        Ok(Reading {
            taken_at,
            snapshot: Snapshot {
                verdict: verdict_from(verdict),
                source,
                confidence: source.confidence(),
                values: self.values(id)?,
            },
        })
    }

    /// The readings of a drive, the newest first, at most `limit`.
    pub fn readings(&self, key: &str, limit: usize) -> Result<Vec<Reading>> {
        let Some(drive) = self.drive_id(key)? else {
            return Ok(Vec::new());
        };
        let mut q = self
            .db
            .prepare(
                "SELECT id, taken_at, verdict, source FROM reading
                 WHERE drive = ?1 ORDER BY taken_at DESC, id DESC LIMIT ?2",
            )
            .map_err(fail)?;
        let rows: Vec<(i64, i64, String, String)> = q
            .query_map(params![drive, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(fail)?
            .collect::<rusqlite::Result<_>>()
            .map_err(fail)?;
        rows.iter()
            .map(|(id, at, v, s)| self.reading_at(*id, *at, v, s))
            .collect()
    }

    /// Store a reading, compare it with the reading before it, and store and
    /// give each alert that the two give.
    pub fn add(&mut self, drive: &Drive, taken_at: i64, snapshot: &Snapshot) -> Result<Vec<Alert>> {
        let before = self.readings(&drive.key, 1)?.into_iter().next();
        let alerts = before
            .map(|b| alert::compare(&b.snapshot, snapshot))
            .unwrap_or_default();

        let tx = self.db.transaction().map_err(fail)?;
        tx.execute(
            "INSERT INTO drive (key, model, serial) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET model = excluded.model",
            params![drive.key, drive.model, drive.serial],
        )
        .map_err(fail)?;
        let drive_id: i64 = tx
            .query_row("SELECT id FROM drive WHERE key = ?1", [&drive.key], |r| {
                r.get(0)
            })
            .map_err(fail)?;
        tx.execute(
            "INSERT INTO reading (drive, taken_at, verdict, source) VALUES (?1, ?2, ?3, ?4)",
            params![
                drive_id,
                taken_at,
                snapshot.verdict.label(),
                snapshot.source.key()
            ],
        )
        .map_err(fail)?;
        let reading = tx.last_insert_rowid();
        {
            let mut put = tx
                .prepare_cached(
                    "INSERT INTO value (reading, position, name, value) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(fail)?;
            for (position, (name, value)) in snapshot.values.iter().enumerate() {
                put.execute(params![reading, position as i64, name, value])
                    .map_err(fail)?;
            }
            let mut put = tx
                .prepare_cached(
                    "INSERT INTO alert (reading, severity, value, message) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(fail)?;
            for a in &alerts {
                put.execute(params![reading, a.severity.label(), a.value, a.message])
                    .map_err(fail)?;
            }
        }
        tx.commit().map_err(fail)?;
        Ok(alerts)
    }

    /// One value of a drive over time, the oldest first: for a chart.
    pub fn series(&self, key: &str, name: &str) -> Result<Vec<(i64, i64)>> {
        let mut q = self
            .db
            .prepare(
                "SELECT r.taken_at, v.value FROM reading r
                 JOIN drive d ON d.id = r.drive
                 JOIN value v ON v.reading = r.id
                 WHERE d.key = ?1 AND v.name = ?2
                 ORDER BY r.taken_at, r.id",
            )
            .map_err(fail)?;
        let rows = q
            .query_map(params![key, name], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(fail)?;
        rows.collect::<rusqlite::Result<_>>().map_err(fail)
    }

    /// The newest alerts of every drive, at most `limit`, the newest first.
    pub fn alerts(&self, limit: usize) -> Result<Vec<StoredAlert>> {
        let mut q = self
            .db
            .prepare(
                "SELECT d.key, d.model, r.taken_at, a.severity, a.value, a.message
                 FROM alert a JOIN reading r ON r.id = a.reading JOIN drive d ON d.id = r.drive
                 ORDER BY r.taken_at DESC, a.id DESC LIMIT ?1",
            )
            .map_err(fail)?;
        let rows = q
            .query_map([limit as i64], |r| {
                let severity: String = r.get(3)?;
                Ok(StoredAlert {
                    drive: r.get(0)?,
                    model: r.get(1)?,
                    taken_at: r.get(2)?,
                    alert: Alert {
                        severity: Severity::from_label(&severity).unwrap_or(Severity::Warning),
                        value: r.get(4)?,
                        message: r.get(5)?,
                    },
                })
            })
            .map_err(fail)?;
        rows.collect::<rusqlite::Result<_>>().map_err(fail)
    }

    /// Each drive that the history knows, with its count of readings and its
    /// last verdict, in the order that they were first seen.
    pub fn drives(&self) -> Result<Vec<Summary>> {
        let mut q = self
            .db
            .prepare(
                "SELECT d.key, d.model, d.serial, COUNT(r.id), MIN(r.taken_at), MAX(r.taken_at),
                    (SELECT verdict FROM reading WHERE drive = d.id
                     ORDER BY taken_at DESC, id DESC LIMIT 1)
                 FROM drive d JOIN reading r ON r.drive = d.id
                 GROUP BY d.id ORDER BY d.id",
            )
            .map_err(fail)?;
        let rows = q
            .query_map([], |r| {
                let verdict: String = r.get(6)?;
                Ok(Summary {
                    drive: Drive {
                        key: r.get(0)?,
                        model: r.get(1)?,
                        serial: r.get(2)?,
                    },
                    readings: r.get(3)?,
                    first_at: r.get(4)?,
                    last_at: r.get(5)?,
                    last_verdict: verdict_from(&verdict),
                })
            })
            .map_err(fail)?;
        rows.collect::<rusqlite::Result<_>>().map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::health::Confidence;

    fn reading(verdict: Verdict, values: &[(&str, i64)]) -> Snapshot {
        Snapshot {
            verdict,
            source: Source::Bridge,
            confidence: Confidence::High,
            values: values.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }
    }

    fn kingston() -> Drive {
        Drive::new(
            "KINGSTON SA400S37240G",
            Some("SERIALMASKED0001"),
            "/dev/sdb",
        )
    }

    #[test]
    fn a_drive_is_known_by_its_model_and_serial_and_else_by_its_node() {
        assert_eq!(kingston().key, "KINGSTON SA400S37240G|SERIALMASKED0001");
        let d = Drive::new(" Stick ", Some("  "), "/dev/sdc");
        assert_eq!(d.key, "Stick|/dev/sdc");
        assert_eq!(d.serial, None);
    }

    // The exit test of P7: a value that changes between two polls makes an
    // alert, and the alert stays in the history.
    #[test]
    fn a_value_that_changes_between_two_polls_makes_an_alert() {
        let mut s = Store::in_memory().unwrap();
        let first = reading(Verdict::Good, &[("ata.5.raw", 0), ("temperature", 40)]);
        assert!(s.add(&kingston(), 1000, &first).unwrap().is_empty());
        let second = reading(Verdict::Good, &[("ata.5.raw", 3), ("temperature", 44)]);
        let alerts = s.add(&kingston(), 2000, &second).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].message, "Reallocated sectors went from 0 to 3.");

        let stored = s.alerts(10).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].taken_at, 2000);
        assert_eq!(stored[0].model, "KINGSTON SA400S37240G");
        assert_eq!(stored[0].alert, alerts[0]);
    }

    #[test]
    fn a_reading_comes_back_as_it_went_in() {
        let mut s = Store::in_memory().unwrap();
        let r = reading(Verdict::Caution, &[("b", 2), ("a", -1), ("c", i64::MAX)]);
        s.add(&kingston(), 5, &r).unwrap();
        let back = s.readings(&kingston().key, 10).unwrap();
        assert_eq!(
            back,
            vec![Reading {
                taken_at: 5,
                snapshot: r
            }]
        );
        assert!(s.readings("nobody", 10).unwrap().is_empty());
    }

    #[test]
    fn each_drive_is_compared_only_with_itself() {
        let mut s = Store::in_memory().unwrap();
        let other = Drive::new("APPLE SSD", Some("SERIALMASKED0002"), "disk0");
        s.add(&kingston(), 1, &reading(Verdict::Good, &[("ata.5.raw", 0)]))
            .unwrap();
        // The other drive has more, and it is not a change of the Kingston.
        assert!(
            s.add(&other, 2, &reading(Verdict::Good, &[("ata.5.raw", 9)]))
                .unwrap()
                .is_empty()
        );
        let summary = s.drives().unwrap();
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0].drive, kingston());
        assert_eq!((summary[1].readings, summary[1].last_at), (1, 2));
    }

    #[test]
    fn a_series_gives_one_value_over_time_the_oldest_first() {
        let mut s = Store::in_memory().unwrap();
        for (at, t) in [(30, 41), (10, 39), (20, 40)] {
            s.add(
                &kingston(),
                at,
                &reading(Verdict::Good, &[("temperature", t)]),
            )
            .unwrap();
        }
        assert_eq!(
            s.series(&kingston().key, "temperature").unwrap(),
            vec![(10, 39), (20, 40), (30, 41)]
        );
        let summary = &s.drives().unwrap()[0];
        assert_eq!(
            (summary.first_at, summary.last_at, summary.readings),
            (10, 30, 3)
        );
    }

    #[test]
    fn a_file_keeps_the_history_after_it_closes() {
        let path = std::env::temp_dir().join(format!("onyx-store-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut s = Store::open(&path).unwrap();
            s.add(&kingston(), 1, &reading(Verdict::Good, &[("ata.5.raw", 0)]))
                .unwrap();
        }
        let mut s = Store::open(&path).unwrap();
        let alerts = s
            .add(&kingston(), 2, &reading(Verdict::Bad, &[("ata.5.raw", 1)]))
            .unwrap();
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].severity, Severity::Critical);
        drop(s);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", path.display()));
        }
    }

    #[test]
    fn a_file_from_a_newer_onyx_is_refused() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version = 99").unwrap();
        let e = Store::from(db).err().unwrap();
        assert!(e.to_string().contains("schema 99"), "{e}");
    }
}
