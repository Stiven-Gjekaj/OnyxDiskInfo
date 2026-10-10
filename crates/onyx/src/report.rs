//! How the health command prints a report.
//!
//! This module makes no system call. It takes a drive and its report and
//! returns the lines, so a test checks the output on any host.

use onyx_core::ata::{self, Attribute};
use onyx_core::health::Assessment;
use onyx_core::nvme::HealthLog;
use onyx_core::{DriveInfo, Report, grouped};

use crate::json::Json;

/// Two columns: a label and its value, with the values lined up.
fn pairs(rows: &[(&str, String)]) -> Vec<String> {
    let width = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(label, value)| {
            let pad = " ".repeat(width - label.chars().count());
            format!("{label}:{pad}  {value}")
        })
        .collect()
}

/// A count for a person to read, with a separator every three digits.
fn count(value: u128) -> String {
    match u64::try_from(value) {
        Ok(v) => grouped(v),
        Err(_) => value.to_string(),
    }
}

/// The lines that `onyx health` prints.
pub fn text(drive: &DriveInfo, report: &Report) -> Vec<String> {
    let assessment = report.assessment();
    let (model, serial, firmware) = report.identity();
    let mut lines = vec![format!(
        "{}  ({}, {})",
        model.unwrap_or(&drive.name),
        drive.id,
        report.protocol()
    )];
    lines.push(String::new());

    let mut summary = vec![("Health", assessment.verdict.label().to_string())];
    if let Some(t) = report.temperature_celsius() {
        summary.push(("Temperature", format!("{t} C")));
    }
    if let Some(h) = report.power_on_hours() {
        summary.push(("Power on", format!("{} hours", count(h))));
    }
    if let Some(c) = report.power_cycles() {
        summary.push(("Power cycles", count(c)));
    }
    summary.push(("Serial", serial.unwrap_or("-").to_string()));
    summary.push(("Firmware", firmware.unwrap_or("-").to_string()));
    lines.extend(pairs(&summary));

    for reason in &assessment.reasons {
        lines.push(format!("  - {reason}"));
    }
    lines.push(String::new());

    match report {
        Report::Nvme { log, .. } => lines.extend(nvme_lines(log)),
        Report::Ata { smart, .. } => lines.extend(ata_lines(&smart.attributes)),
        _ => {}
    }
    lines
}

fn nvme_lines(log: &HealthLog) -> Vec<String> {
    let rows = [
        ("Critical warning", format!("{:#04x}", log.critical_warning)),
        ("Available spare", format!("{}%", log.available_spare)),
        (
            "Spare threshold",
            format!("{}%", log.available_spare_threshold),
        ),
        ("Percentage used", format!("{}%", log.percentage_used)),
        ("Data units read", count(log.data_units_read)),
        ("Data units written", count(log.data_units_written)),
        ("Host read commands", count(log.host_read_commands)),
        ("Host write commands", count(log.host_write_commands)),
        (
            "Controller busy",
            format!("{} minutes", count(log.controller_busy_minutes)),
        ),
        ("Unsafe shutdowns", count(log.unsafe_shutdowns)),
        ("Media errors", count(log.media_errors)),
        ("Error log entries", count(log.error_log_entries)),
    ];
    pairs(&rows)
}

fn ata_lines(attributes: &[Attribute]) -> Vec<String> {
    let mut rows: Vec<[String; 6]> =
        vec![["ID", "ATTRIBUTE", "CURRENT", "WORST", "THRESHOLD", "RAW"].map(str::to_string)];
    for a in attributes {
        rows.push([
            a.id.to_string(),
            ata::attribute_name(a.id).unwrap_or("-").to_string(),
            a.current.to_string(),
            a.worst.to_string(),
            a.threshold.map_or("-".to_string(), |t| t.to_string()),
            a.value().to_string(),
        ]);
    }
    let mut width = [0usize; 6];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            width[column] = width[column].max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (column, cell) in row.iter().enumerate() {
                if column > 0 {
                    line.push_str("  ");
                }
                line.push_str(cell);
                if column + 1 < row.len() {
                    line.push_str(&" ".repeat(width[column] - cell.chars().count()));
                }
            }
            line
        })
        .collect()
}

fn assessment_json(a: &Assessment) -> Json {
    Json::Object(vec![
        ("verdict", Json::text(a.verdict.label())),
        (
            "reasons",
            Json::List(a.reasons.iter().map(Json::text).collect()),
        ),
    ])
}

fn maybe_count(value: Option<u128>) -> Json {
    value.map_or(Json::Null, Json::Number)
}

/// The report, as `onyx health --json` prints it.
pub fn json(drive: &DriveInfo, report: &Report) -> Json {
    let (model, serial, firmware) = report.identity();
    let mut members = vec![
        ("id", Json::text(drive.id.as_str())),
        ("protocol", Json::text(report.protocol())),
        ("model", Json::maybe_text(model)),
        ("serial", Json::maybe_text(serial)),
        ("firmware", Json::maybe_text(firmware)),
        ("health", assessment_json(&report.assessment())),
        (
            "temperature_celsius",
            report
                .temperature_celsius()
                .map_or(Json::Null, |t| Json::Signed(t.into())),
        ),
        ("power_on_hours", maybe_count(report.power_on_hours())),
        ("power_cycles", maybe_count(report.power_cycles())),
    ];
    match report {
        Report::Nvme { log, .. } => members.push(("nvme", nvme_json(log))),
        Report::Ata { smart, .. } => members.push((
            "attributes",
            Json::List(smart.attributes.iter().map(attribute_json).collect()),
        )),
        _ => {}
    }
    Json::Object(members)
}

fn nvme_json(log: &HealthLog) -> Json {
    Json::Object(vec![
        (
            "critical_warning",
            Json::Number(log.critical_warning.into()),
        ),
        ("available_spare", Json::Number(log.available_spare.into())),
        (
            "available_spare_threshold",
            Json::Number(log.available_spare_threshold.into()),
        ),
        ("percentage_used", Json::Number(log.percentage_used.into())),
        ("data_units_read", Json::Number(log.data_units_read)),
        ("data_units_written", Json::Number(log.data_units_written)),
        ("host_read_commands", Json::Number(log.host_read_commands)),
        ("host_write_commands", Json::Number(log.host_write_commands)),
        (
            "controller_busy_minutes",
            Json::Number(log.controller_busy_minutes),
        ),
        ("unsafe_shutdowns", Json::Number(log.unsafe_shutdowns)),
        ("media_errors", Json::Number(log.media_errors)),
        ("error_log_entries", Json::Number(log.error_log_entries)),
    ])
}

fn attribute_json(a: &Attribute) -> Json {
    Json::Object(vec![
        ("id", Json::Number(a.id.into())),
        ("name", Json::maybe_text(ata::attribute_name(a.id))),
        ("current", Json::Number(a.current.into())),
        ("worst", Json::Number(a.worst.into())),
        (
            "threshold",
            a.threshold.map_or(Json::Null, |t| Json::Number(t.into())),
        ),
        ("raw", Json::Number(a.raw48().into())),
        ("value", Json::Number(a.value().into())),
        ("failing_now", Json::Bool(a.failing_now())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::{DriveId, DriveInfo};

    fn drive() -> DriveInfo {
        DriveInfo::new(DriveId::new("disk0"), "/dev/rdisk0", "APPLE SSD")
    }

    /// An NVMe report with a few values set, built from the layout.
    fn nvme_report() -> Report {
        let mut identify = vec![0u8; 4096];
        identify[4..12].copy_from_slice(b"SERIAL01");
        identify[24..33].copy_from_slice(b"Some SSD ");
        identify[64..68].copy_from_slice(b"1.00");
        let mut log = vec![0u8; 512];
        log[1..3].copy_from_slice(&308u16.to_le_bytes());
        log[3] = 100;
        log[4] = 10;
        log[112] = 195;
        log[128] = 111;
        Report::nvme(&identify, &log).unwrap()
    }

    #[test]
    fn the_text_names_the_drive_and_gives_the_verdict_first() {
        let lines = text(&drive(), &nvme_report());
        assert_eq!(lines[0], "Some SSD  (disk0, NVMe)");
        assert_eq!(lines[2], "Health:        Good");
        assert!(
            lines.iter().any(|l| l == "Temperature:   35 C"),
            "{lines:#?}"
        );
        assert!(lines.iter().any(|l| l == "Power on:      111 hours"));
        assert!(lines.iter().any(|l| l == "Serial:        SERIAL01"));
    }

    #[test]
    fn each_reason_follows_the_summary() {
        let mut identify = vec![0u8; 4096];
        identify[24..27].copy_from_slice(b"SSD");
        let mut log = vec![0u8; 512];
        log[0] = onyx_core::nvme::critical::READ_ONLY;
        let report = Report::nvme(&identify, &log).unwrap();
        let lines = text(&drive(), &report);
        assert!(
            lines.iter().any(|l| l == "Health:        Bad"),
            "{lines:#?}"
        );
        assert!(lines.iter().any(|l| l.starts_with("  - The drive put")));
    }

    #[test]
    fn the_json_holds_the_verdict_and_each_counter() {
        let json = json(&drive(), &nvme_report()).to_string();
        assert!(json.starts_with(r#"{"id":"disk0","protocol":"NVMe","model":"Some SSD","#));
        assert!(json.contains(r#""health":{"verdict":"Good","reasons":[]}"#));
        assert!(json.contains(r#""temperature_celsius":35"#));
        assert!(json.contains(r#""power_on_hours":111"#));
        assert!(json.contains(r#""available_spare_threshold":10"#));
    }

    fn ata_report() -> Report {
        let mut identify = vec![0u8; 512];
        // "MODEL" in word order: each word holds two characters, high first.
        identify[54..60].copy_from_slice(b"OMED L");
        let mut data = vec![0u8; 512];
        let entry = [194u8, 0x22, 0, 64, 60, 36, 0, 20, 0, 52, 0];
        data[2..13].copy_from_slice(&entry);
        let mut limits = vec![0u8; 512];
        limits[2] = 194;
        let sum = data[..511].iter().fold(0u8, |s, b| s.wrapping_add(*b));
        data[511] = 0u8.wrapping_sub(sum);
        Report::ata(&identify, &data, Some(&limits)).unwrap()
    }

    #[test]
    fn an_ata_report_prints_its_attribute_table() {
        let lines = text(&drive(), &ata_report());
        assert_eq!(lines[0], "MODEL  (disk0, ATA)");
        let table: Vec<&String> = lines.iter().skip_while(|l| !l.starts_with("ID")).collect();
        assert!(table[0].contains("ATTRIBUTE") && table[0].ends_with("RAW"));
        assert!(table[1].starts_with("194  Temperature"), "{}", table[1]);
        assert!(table[1].ends_with("  36"), "{}", table[1]);
    }

    #[test]
    fn an_ata_report_as_json_lists_each_attribute() {
        let json = json(&drive(), &ata_report()).to_string();
        assert!(json.contains(r#""attributes":[{"id":194,"name":"Temperature","current":64,"worst":60,"threshold":0,"#), "{json}");
        assert!(
            json.contains(r#""value":36,"failing_now":false}"#),
            "{json}"
        );
    }
}
