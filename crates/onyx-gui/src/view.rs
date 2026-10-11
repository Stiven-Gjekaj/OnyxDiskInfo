//! What the window shows for one drive, as plain values.
//!
//! The view comes from the JSON of `onyx list --json` and `onyx health
//! --json`. The app makes that JSON itself for a drive that it can read, and
//! runs the command line with administrator rights for a drive that it
//! cannot. Both ways end here, so the window shows what the command line
//! prints, and nothing else.

use serde_json::Value;

/// The verdict, as the window colors it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Good,
    Caution,
    Bad,
    /// No health data yet, or none at all.
    Unknown,
}

/// One row of the attribute table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub current: String,
    pub worst: String,
    pub threshold: String,
    pub raw: String,
    /// The row is the reason for a caution or a bad verdict.
    pub warn: bool,
}

/// Everything that the window shows for one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveView {
    /// The number of the drive in `onyx list`.
    pub number: u64,
    pub name: String,
    pub verdict: Verdict,
    /// The word of the verdict, or a word for why there is none.
    pub verdict_text: String,
    pub confidence: String,
    pub source: String,
    pub temperature: String,
    /// The color of the temperature badge.
    pub temperature_verdict: Verdict,
    /// Label and value pairs for the grid beside the badges.
    pub info: Vec<(String, String)>,
    pub reasons: Vec<String>,
    /// The column names of the table.
    pub columns: [String; 6],
    pub rows: Vec<Row>,
    /// A message instead of health data, such as an error.
    pub message: String,
    /// The drive needs administrator rights to read.
    pub needs_rights: bool,
    /// The temperature in the history, as the commands of a path in a box
    /// of [`TREND_WIDTH`] by [`TREND_HEIGHT`]. Empty when the history holds
    /// fewer than two readings.
    pub trend_commands: String,
    /// What the chart shows, in words.
    pub trend_text: String,
    /// The newest alerts of the drive in the history.
    pub alerts: Vec<String>,
}

/// The size of the box that the commands of the chart draw in.
pub const TREND_WIDTH: f64 = 1000.0;
pub const TREND_HEIGHT: f64 = 100.0;

/// The commands of a line through `points`, each a time and a value, that
/// fills the box from the first time to the last and from the lowest value
/// to the highest. A flat line goes through the middle.
pub fn trend_commands(points: &[(i64, i64)]) -> String {
    if points.len() < 2 {
        return String::new();
    }
    let (t0, t1) = (points[0].0, points[points.len() - 1].0);
    let low = points.iter().map(|p| p.1).min().unwrap_or(0);
    let high = points.iter().map(|p| p.1).max().unwrap_or(0);
    let x = |t: i64| {
        if t1 == t0 {
            0.0
        } else {
            (t - t0) as f64 / (t1 - t0) as f64 * TREND_WIDTH
        }
    };
    let y = |v: i64| {
        if high == low {
            TREND_HEIGHT / 2.0
        } else {
            TREND_HEIGHT - (v - low) as f64 / (high - low) as f64 * TREND_HEIGHT
        }
    };
    points
        .iter()
        .enumerate()
        .map(|(i, (t, v))| {
            format!(
                "{} {:.1} {:.1}",
                if i == 0 { "M" } else { "L" },
                x(*t),
                y(*v)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words over the chart.
pub fn trend_text(points: &[(i64, i64)]) -> String {
    match points {
        [] => "No temperature in the history yet.".to_string(),
        [one] => format!("Temperature: {} C, one reading so far.", one.1),
        _ => {
            let low = points.iter().map(|p| p.1).min().unwrap_or(0);
            let high = points.iter().map(|p| p.1).max().unwrap_or(0);
            format!(
                "Temperature: {low} to {high} C over {} readings since {}",
                points.len(),
                onyx::history::utc(points[0].0)
            )
        }
    }
}

/// The view with the history of the drive.
pub fn with_history(
    mut view: DriveView,
    temperature: &[(i64, i64)],
    alerts: Vec<String>,
) -> DriveView {
    view.trend_commands = trend_commands(temperature);
    view.trend_text = trend_text(temperature);
    view.alerts = alerts;
    view
}

fn text(json: &Value, key: &str) -> Option<String> {
    json.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(json: &Value, key: &str) -> Option<u64> {
    json.get(key).and_then(Value::as_u64)
}

fn or_dash(value: Option<String>) -> String {
    value.unwrap_or_else(|| "-".to_string())
}

/// A count with a separator every three digits.
fn grouped(value: u64) -> String {
    onyx_core::grouped(value)
}

/// The view of a drive before its health arrives, from one entry of
/// `onyx list --json`.
pub fn listed(drive: &Value) -> DriveView {
    let size = number(drive, "size_bytes").unwrap_or(0);
    let info = vec![
        (
            "Model".to_string(),
            or_dash(text(drive, "model").or(text(drive, "name"))),
        ),
        ("Firmware".to_string(), or_dash(text(drive, "firmware"))),
        ("Serial".to_string(), or_dash(text(drive, "serial"))),
        (
            "Interface".to_string(),
            format!(
                "{} ({})",
                or_dash(text(drive, "bus")),
                or_dash(text(drive, "connection"))
            ),
        ),
        (
            "Size".to_string(),
            format!("{} ({} bytes)", onyx_core::human_size(size), grouped(size)),
        ),
        ("Drive".to_string(), or_dash(text(drive, "id"))),
    ];
    DriveView {
        number: number(drive, "number").unwrap_or(0),
        name: or_dash(text(drive, "name")),
        verdict: Verdict::Unknown,
        verdict_text: "Reading".to_string(),
        confidence: "-".to_string(),
        source: "-".to_string(),
        temperature: "-- C".to_string(),
        temperature_verdict: Verdict::Unknown,
        info,
        reasons: Vec::new(),
        columns: ["ID", "Attribute", "Current", "Worst", "Threshold", "Raw"].map(str::to_string),
        rows: Vec::new(),
        message: String::new(),
        needs_rights: false,
        trend_commands: String::new(),
        trend_text: String::new(),
        alerts: Vec::new(),
    }
}

/// The color of a temperature, as CrystalDiskInfo colors it: good below
/// 50 C, caution from 50 C, and bad from 60 C. A drive can run hotter than
/// this within its rating, so the color is a hint and not a verdict on the
/// drive.
pub fn temperature_verdict(celsius: Option<i64>) -> Verdict {
    match celsius {
        None => Verdict::Unknown,
        Some(t) if t >= 60 => Verdict::Bad,
        Some(t) if t >= 50 => Verdict::Caution,
        Some(_) => Verdict::Good,
    }
}

fn verdict(word: &str) -> Verdict {
    match word {
        "Good" => Verdict::Good,
        "Caution" => Verdict::Caution,
        "Bad" => Verdict::Bad,
        _ => Verdict::Unknown,
    }
}

/// The rows of an NVMe health log. NVMe has no current, worst, or threshold
/// values, so those columns stay empty and the value goes to the raw column,
/// as in CrystalDiskInfo.
fn nvme_rows(nvme: &Value) -> Vec<Row> {
    const FIELDS: [(&str, &str, &str); 12] = [
        ("01", "Critical Warning", "critical_warning"),
        ("02", "Available Spare", "available_spare"),
        (
            "03",
            "Available Spare Threshold",
            "available_spare_threshold",
        ),
        ("04", "Percentage Used", "percentage_used"),
        ("05", "Data Units Read", "data_units_read"),
        ("06", "Data Units Written", "data_units_written"),
        ("07", "Host Read Commands", "host_read_commands"),
        ("08", "Host Write Commands", "host_write_commands"),
        ("09", "Controller Busy Time", "controller_busy_minutes"),
        ("0A", "Unsafe Shutdowns", "unsafe_shutdowns"),
        ("0B", "Media and Data Integrity Errors", "media_errors"),
        ("0C", "Error Log Entries", "error_log_entries"),
    ];
    FIELDS
        .iter()
        .map(|(id, name, key)| {
            let value = number(nvme, key);
            let warn = match *key {
                "critical_warning" | "media_errors" => value.is_some_and(|v| v > 0),
                _ => false,
            };
            Row {
                id: id.to_string(),
                name: name.to_string(),
                raw: value.map_or("-".to_string(), grouped),
                warn,
                ..Row::default()
            }
        })
        .collect()
}

fn ata_rows(attributes: &[Value]) -> Vec<Row> {
    attributes
        .iter()
        .map(|a| {
            let n = |key| number(a, key).map_or("-".to_string(), |v| v.to_string());
            Row {
                id: number(a, "id").map_or("-".to_string(), |v| format!("{v:02X}")),
                name: or_dash(text(a, "name")),
                current: n("current"),
                worst: n("worst"),
                threshold: n("threshold"),
                raw: number(a, "value").map_or("-".to_string(), grouped),
                warn: a.get("failing_now").and_then(Value::as_bool) == Some(true),
            }
        })
        .collect()
}

fn measured_rows(m: &Value) -> Vec<Row> {
    let speed = number(m, "median_bytes_per_second").map_or("-".to_string(), |b| {
        format!("{}/s", onyx_core::human_size(b))
    });
    let slowest = number(m, "max_micros").map_or("-".to_string(), |t| format!("{} ms", t / 1000));
    let errors = m
        .get("errors")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let row = |name: &str, raw: String, warn: bool| Row {
        name: name.to_string(),
        raw,
        warn,
        ..Row::default()
    };
    vec![
        row(
            "Reads",
            number(m, "reads").map_or("-".to_string(), |v| v.to_string()),
            false,
        ),
        row("Failed Reads", errors.to_string(), errors > 0),
        row("Median Speed", speed, false),
        row("Slowest Read", slowest, false),
    ]
}

/// Add the health of a drive, from `onyx health --json`, to its view.
pub fn with_health(mut view: DriveView, health: &Value) -> DriveView {
    let assessment = health.get("health").cloned().unwrap_or(Value::Null);
    let word = or_dash(text(&assessment, "verdict"));
    view.verdict = verdict(&word);
    view.verdict_text = word;
    view.confidence = or_dash(text(health, "confidence"));
    view.source = match text(health, "source").as_deref() {
        Some("drive") => "the drive".to_string(),
        Some("bridge") => "the drive, through a USB bridge".to_string(),
        Some("smartctl") => "smartctl".to_string(),
        Some("measured") => "measured by Onyx".to_string(),
        other => or_dash(other.map(str::to_string)),
    };
    let celsius = health.get("temperature_celsius").and_then(Value::as_i64);
    view.temperature = celsius.map_or("-- C".to_string(), |t| format!("{t} C"));
    view.temperature_verdict = temperature_verdict(celsius);
    view.reasons = assessment
        .get("reasons")
        .and_then(Value::as_array)
        .map(|r| {
            r.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // The drive names itself better than the host list does.
    for (key, label) in [
        ("model", "Model"),
        ("firmware", "Firmware"),
        ("serial", "Serial"),
    ] {
        if let Some(value) = text(health, key)
            && let Some(row) = view.info.iter_mut().find(|(l, _)| l == label)
        {
            row.1 = value;
        }
    }
    if let Some(protocol) = text(health, "protocol") {
        view.info.push(("Protocol".to_string(), protocol));
    }
    if let Some(hours) = number(health, "power_on_hours") {
        view.info.push((
            "Power On Hours".to_string(),
            format!("{} hours", grouped(hours)),
        ));
    }
    if let Some(cycles) = number(health, "power_cycles") {
        view.info.push((
            "Power On Count".to_string(),
            format!("{} count", grouped(cycles)),
        ));
    }

    if let Some(nvme) = health.get("nvme") {
        view.rows = nvme_rows(nvme);
    } else if let Some(attributes) = health.get("attributes").and_then(Value::as_array) {
        view.rows = ata_rows(attributes);
    } else if let Some(m) = health.get("measurement") {
        view.rows = measured_rows(m);
    }
    view.message.clear();
    view.needs_rights = false;
    view
}

/// The message that the window shows for an error of the command line.
///
/// The command line tells a person at a terminal to use sudo. In the window,
/// the button does that.
pub fn error_message(error: &onyx_core::Error) -> (String, bool) {
    match error {
        onyx_core::Error::NeedsPrivilege { .. } => (
            "Onyx needs administrator rights to read this drive.".to_string(),
            true,
        ),
        other => (other.to_string(), false),
    }
}

/// Mark a view with an error in place of its health.
///
/// A drive that needs administrator rights gets a button that asks for them.
/// A drive that gives no health data at all says so.
pub fn with_error(mut view: DriveView, message: &str, needs_rights: bool) -> DriveView {
    view.verdict = Verdict::Unknown;
    view.verdict_text = if needs_rights { "Locked" } else { "Unknown" }.to_string();
    view.message = message.to_string();
    view.needs_rights = needs_rights;
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apple() -> Value {
        serde_json::from_str(
            r#"{"number":1,"id":"disk0","node":"/dev/rdisk0","name":"APPLE SSD AP0512Z","vendor":null,
            "model":"APPLE SSD AP0512Z","serial":"SERIALMASKED0001","firmware":"241.0.12",
            "size_bytes":500277792768,"logical_sector_size":4096,"physical_sector_size":null,
            "bus":"Fabric","connection":"internal","removable":false,"system":true,"read_only":false}"#,
        )
        .unwrap()
    }

    fn apple_health() -> Value {
        serde_json::from_str(
            r#"{"id":"disk0","source":"drive","confidence":"high","health":{"verdict":"Good","reasons":[]},
            "protocol":"NVMe","model":"APPLE SSD AP0512Z","serial":"SERIALMASKED0001","firmware":"241.0.12",
            "temperature_celsius":30,"power_on_hours":112,"power_cycles":195,
            "nvme":{"critical_warning":0,"available_spare":100,"available_spare_threshold":99,
            "percentage_used":0,"data_units_read":35268996,"data_units_written":17056320,
            "host_read_commands":770372360,"host_write_commands":236205249,"controller_busy_minutes":0,
            "unsafe_shutdowns":11,"media_errors":0,"error_log_entries":0}}"#,
        )
        .unwrap()
    }

    fn info<'a>(view: &'a DriveView, label: &str) -> &'a str {
        &view.info.iter().find(|(l, _)| l == label).unwrap().1
    }

    #[test]
    fn a_listed_drive_shows_its_names_and_waits_for_its_health() {
        let v = listed(&apple());
        assert_eq!(v.number, 1);
        assert_eq!(v.verdict, Verdict::Unknown);
        assert_eq!(v.verdict_text, "Reading");
        assert_eq!(info(&v, "Size"), "500.3 GB (500,277,792,768 bytes)");
        assert_eq!(info(&v, "Interface"), "Fabric (internal)");
        assert!(v.rows.is_empty());
    }

    #[test]
    fn the_health_of_an_nvme_drive_fills_the_badges_and_the_table() {
        let v = with_health(listed(&apple()), &apple_health());
        assert_eq!(v.verdict, Verdict::Good);
        assert_eq!(v.verdict_text, "Good");
        assert_eq!(v.confidence, "high");
        assert_eq!(v.source, "the drive");
        assert_eq!(v.temperature, "30 C");
        assert_eq!(info(&v, "Power On Hours"), "112 hours");
        assert_eq!(info(&v, "Power On Count"), "195 count");
        assert_eq!(info(&v, "Protocol"), "NVMe");
        assert_eq!(v.rows.len(), 12);
        assert_eq!(v.rows[4].name, "Data Units Read");
        assert_eq!(v.rows[4].raw, "35,268,996");
        assert!(v.rows.iter().all(|r| !r.warn));
    }

    #[test]
    fn a_media_error_marks_its_row() {
        let mut h = apple_health();
        h["nvme"]["media_errors"] = 3.into();
        let v = with_health(listed(&apple()), &h);
        assert!(v.rows[10].warn);
    }

    #[test]
    fn an_ata_drive_gives_one_row_for_each_attribute() {
        let h: Value = serde_json::from_str(
            r#"{"source":"bridge","confidence":"high","health":{"verdict":"Caution","reasons":["Attribute 5 (Reallocated Sectors Count) counts 8."]},
            "protocol":"ATA","temperature_celsius":36,
            "attributes":[{"id":5,"name":"Reallocated Sectors Count","current":100,"worst":100,"threshold":10,"raw":8,"value":8,"failing_now":false},
            {"id":194,"name":"Temperature","current":64,"worst":48,"threshold":0,"raw":223339610148,"value":36,"failing_now":false}]}"#,
        )
        .unwrap();
        let v = with_health(listed(&apple()), &h);
        assert_eq!(v.verdict, Verdict::Caution);
        assert_eq!(v.source, "the drive, through a USB bridge");
        assert_eq!(v.reasons.len(), 1);
        assert_eq!(v.rows[0].id, "05");
        assert_eq!(v.rows[1].id, "C2");
        assert_eq!(v.rows[1].raw, "36");
        assert_eq!(v.rows[0].threshold, "10");
    }

    #[test]
    fn a_measured_drive_shows_its_sample() {
        let h: Value = serde_json::from_str(
            r#"{"source":"measured","confidence":"low","health":{"verdict":"Good","reasons":[]},
            "measurement":{"reads":64,"chunk_bytes":1048576,"median_bytes_per_second":21325089,"max_micros":50866,"errors":[]}}"#,
        )
        .unwrap();
        let v = with_health(listed(&apple()), &h);
        assert_eq!(v.confidence, "low");
        assert_eq!(v.source, "measured by Onyx");
        assert_eq!(v.temperature, "-- C");
        assert_eq!(v.rows[0].raw, "64");
        assert_eq!(v.rows[2].raw, "21.3 MB/s");
        assert_eq!(v.rows[3].raw, "50 ms");
    }

    #[test]
    fn the_temperature_takes_the_colors_of_crystaldiskinfo() {
        assert_eq!(temperature_verdict(None), Verdict::Unknown);
        assert_eq!(temperature_verdict(Some(49)), Verdict::Good);
        assert_eq!(temperature_verdict(Some(50)), Verdict::Caution);
        assert_eq!(temperature_verdict(Some(59)), Verdict::Caution);
        assert_eq!(temperature_verdict(Some(60)), Verdict::Bad);
        let v = with_health(listed(&apple()), &apple_health());
        assert_eq!(v.temperature_verdict, Verdict::Good);
    }

    #[test]
    fn missing_rights_get_the_words_of_the_window_and_the_button() {
        let (text, button) = error_message(&onyx_core::Error::NeedsPrivilege {
            what: "/dev/rdisk4".to_string(),
        });
        assert!(button);
        assert!(!text.contains("sudo"), "{text}");
        let (text, button) = error_message(&onyx_core::Error::NoHealthData {
            name: "Stick".to_string(),
            reason: "none".to_string(),
        });
        assert!(!button);
        assert_eq!(text, "Stick gives no health data: none");
    }

    #[test]
    fn a_drive_that_needs_rights_is_locked_and_offers_the_button() {
        let v = with_error(listed(&apple()), "needs administrator rights", true);
        assert_eq!(v.verdict, Verdict::Unknown);
        assert_eq!(v.verdict_text, "Locked");
        assert!(v.needs_rights);
        let v = with_error(listed(&apple()), "gives no health data", false);
        assert_eq!(v.verdict_text, "Unknown");
        assert!(!v.needs_rights);
    }

    #[test]
    fn the_chart_runs_from_the_first_reading_to_the_last_and_fills_the_box() {
        let c = trend_commands(&[(100, 30), (200, 40), (300, 35)]);
        assert_eq!(c, "M 0.0 100.0 L 500.0 0.0 L 1000.0 50.0");
        assert_eq!(
            trend_commands(&[(1, 40), (2, 40)]),
            "M 0.0 50.0 L 1000.0 50.0"
        );
        assert_eq!(trend_commands(&[(1, 40)]), "");
    }

    #[test]
    fn the_words_of_the_chart_give_the_range_and_the_count() {
        assert_eq!(
            trend_text(&[(0, 31), (60, 45)]),
            "Temperature: 31 to 45 C over 2 readings since 1970-01-01 00:00 UTC"
        );
        assert!(trend_text(&[(0, 31)]).contains("one reading"));
        assert!(trend_text(&[]).contains("No temperature"));
    }
}
