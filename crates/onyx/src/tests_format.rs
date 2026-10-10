//! How the disk test commands print their results.
//!
//! This module makes no system call. It takes a report and returns lines or
//! JSON, so a test checks the output on any host.

use onyx_bench::bench::JobResult;
use onyx_bench::capacity::{CapacityReport, Fault};
use onyx_bench::scan::{self, ScanReport};
use onyx_bench::verify::{Bad, VerifyReport};
use onyx_core::{DriveInfo, grouped, human_size};

use crate::json::Json;

/// The most entries of a list that the text prints. JSON holds every entry
/// that the report kept.
const SHOWN: usize = 20;

/// A time in microseconds, as a person reads it.
pub fn duration(micros: u64) -> String {
    let seconds = micros / 1_000_000;
    match seconds {
        0 => format!("{} ms", micros / 1000),
        1..60 => format!("{}.{} s", seconds, micros % 1_000_000 / 100_000),
        60..3600 => format!("{} min {} s", seconds / 60, seconds % 60),
        _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// A speed, as a person reads it.
pub fn speed(bytes_per_second: u64) -> String {
    format!("{}/s", human_size(bytes_per_second))
}

fn heading(what: &str, drive: &DriveInfo) -> String {
    format!(
        "{what} of {} ({}), {}",
        drive.name,
        drive.id,
        human_size(drive.size_bytes)
    )
}

/// The names of the time classes of a scan.
fn class_names() -> Vec<String> {
    let mut names: Vec<String> = scan::LIMITS_MS
        .iter()
        .map(|ms| format!("< {ms} ms"))
        .collect();
    names.push(format!(
        ">= {} ms",
        scan::LIMITS_MS[scan::LIMITS_MS.len() - 1]
    ));
    names
}

pub fn scan_text(drive: &DriveInfo, r: &ScanReport) -> Vec<String> {
    let mut lines = vec![heading("Surface scan", drive), String::new()];
    let names = class_names();
    for (name, count) in names.iter().zip(r.classes) {
        lines.push(format!("{name:<10}  {}", grouped(count)));
    }
    lines.push(format!("{:<10}  {}", "failed", grouped(r.failed_blocks)));
    lines.push(String::new());
    lines.push(format!(
        "{} blocks of {} in {}, at {}.",
        grouped(r.blocks()),
        human_size(r.block_bytes),
        duration(r.micros),
        speed(r.bytes_per_second())
    ));
    if !r.complete {
        lines.push("The scan stopped before the end of the drive.".to_string());
    }
    for (offset, message) in r.failed.iter().take(SHOWN) {
        lines.push(format!("  failed at byte {}: {message}", grouped(*offset)));
    }
    for (offset, micros) in r.slow.iter().take(SHOWN) {
        lines.push(format!(
            "  slow at byte {}: {} ms",
            grouped(*offset),
            micros / 1000
        ));
    }
    lines.push(String::new());
    lines.push(match (r.failed_blocks, r.slow.len()) {
        (0, 0) => "Each block read, and none was slow.".to_string(),
        (0, slow) => format!("Each block read. {slow} blocks took 200 ms or more."),
        (failed, _) => format!("{failed} blocks did not read. The drive is losing data."),
    });
    lines
}

pub fn scan_json(drive: &DriveInfo, r: &ScanReport) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("block_bytes", Json::Number(r.block_bytes.into())),
        ("blocks", Json::Number(r.blocks().into())),
        (
            "classes",
            Json::List(
                class_names()
                    .into_iter()
                    .zip(r.classes)
                    .map(|(name, count)| {
                        Json::Object(vec![
                            ("class", Json::text(name)),
                            ("blocks", Json::Number(count.into())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("failed_blocks", Json::Number(r.failed_blocks.into())),
        (
            "failed",
            Json::List(
                r.failed
                    .iter()
                    .map(|(o, m)| {
                        Json::Object(vec![
                            ("offset", Json::Number((*o).into())),
                            ("message", Json::text(m)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "slow",
            Json::List(
                r.slow
                    .iter()
                    .map(|(o, t)| {
                        Json::Object(vec![
                            ("offset", Json::Number((*o).into())),
                            ("micros", Json::Number((*t).into())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "bytes_per_second",
            Json::Number(r.bytes_per_second().into()),
        ),
        ("micros", Json::Number(r.micros.into())),
        ("complete", Json::Bool(r.complete)),
    ])
}

/// The benchmark table: one line for each job, with the read and, when
/// there is one, the write.
pub fn bench_text(what: &str, reads: &[JobResult], writes: &[JobResult]) -> Vec<String> {
    let mut rows: Vec<Vec<String>> = vec![vec![
        "JOB".to_string(),
        "READ".to_string(),
        "IOPS".to_string(),
        "LATENCY".to_string(),
    ]];
    if !writes.is_empty() {
        rows[0].extend(["WRITE", "IOPS", "LATENCY"].map(str::to_string));
    }
    for (index, read) in reads.iter().enumerate() {
        let mut row = vec![
            read.job.name.to_string(),
            speed(read.bytes_per_second()),
            grouped(read.iops()),
            format!("{} us", grouped(read.mean_latency_micros())),
        ];
        if let Some(write) = writes.get(index) {
            row.push(speed(write.bytes_per_second()));
            row.push(grouped(write.iops()));
            row.push(format!("{} us", grouped(write.mean_latency_micros())));
        }
        rows.push(row);
    }
    let columns = rows[0].len();
    let mut width = vec![0usize; columns];
    for row in &rows {
        for (c, cell) in row.iter().enumerate() {
            width[c] = width[c].max(cell.chars().count());
        }
    }
    let mut lines = vec![what.to_string(), String::new()];
    for row in &rows {
        let mut line = String::new();
        for (c, cell) in row.iter().enumerate() {
            if c > 0 {
                line.push_str("  ");
            }
            // The name goes left, and each number goes right.
            let pad = " ".repeat(width[c] - cell.chars().count());
            if c == 0 {
                line.push_str(cell);
                line.push_str(&pad);
            } else {
                line.push_str(&pad);
                line.push_str(cell);
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn job_json(r: &JobResult) -> Json {
    Json::Object(vec![
        ("job", Json::text(r.job.name)),
        ("write", Json::Bool(r.job.write)),
        (
            "bytes_per_second",
            Json::Number(r.bytes_per_second().into()),
        ),
        ("iops", Json::Number(r.iops().into())),
        (
            "mean_latency_micros",
            Json::Number(r.mean_latency_micros().into()),
        ),
        ("bytes", Json::Number(r.bytes.into())),
        ("commands", Json::Number(r.commands.into())),
        ("micros", Json::Number(r.micros.into())),
    ])
}

pub fn bench_json(target: &str, reads: &[JobResult], writes: &[JobResult]) -> Json {
    Json::Object(vec![
        ("target", Json::text(target)),
        (
            "jobs",
            Json::List(reads.iter().chain(writes).map(job_json).collect()),
        ),
    ])
}

fn fault_text(fault: &Fault) -> String {
    match fault {
        Fault::Wraps { onto } => format!(
            "it reads back with the data of byte {}, so the drive wraps",
            grouped(*onto)
        ),
        Fault::Dropped => "it reads back as data that the test did not write".to_string(),
        Fault::ReadError(e) => format!("the read failed: {e}"),
        Fault::WriteError(e) => format!("the write failed: {e}"),
    }
}

pub fn capacity_text(drive: &DriveInfo, r: &CapacityReport) -> Vec<String> {
    let mut lines = vec![heading("Capacity test", drive), String::new()];
    lines.push(format!(
        "Claimed:   {} ({} bytes)",
        human_size(r.claimed),
        grouped(r.claimed)
    ));
    lines.push(format!("Probes:    {}", r.probes));
    lines.push(format!("Faults:    {}", r.faults));
    lines.push(format!(
        "Restored:  {}",
        if r.restored {
            "yes, each saved block went back"
        } else {
            "no. Some saved blocks did not go back"
        }
    ));
    lines.push(String::new());
    match &r.first_fault {
        None => lines.push(format!(
            "Genuine. Each probe holds what the test wrote, up to the claimed {}.",
            human_size(r.claimed)
        )),
        Some((offset, fault)) => {
            lines.push(format!(
                "FAKE. The drive holds at least {} and less than {}.",
                human_size(r.proven_bytes),
                human_size(*offset + onyx_bench::capacity::PROBE_BYTES)
            ));
            lines.push(format!(
                "The first fault is at byte {}: {}.",
                grouped(*offset),
                fault_text(fault)
            ));
            lines.push(
                "Data past the real end is lost. Format the drive to the real size, or stop using it."
                    .to_string(),
            );
        }
    }
    lines
}

pub fn capacity_json(drive: &DriveInfo, r: &CapacityReport) -> Json {
    let (offset, kind, detail) = match &r.first_fault {
        None => (Json::Null, Json::Null, Json::Null),
        Some((offset, fault)) => {
            let (kind, detail) = match fault {
                Fault::Wraps { onto } => ("wraps", Json::Number((*onto).into())),
                Fault::Dropped => ("dropped", Json::Null),
                Fault::ReadError(e) => ("read_error", Json::text(e)),
                Fault::WriteError(e) => ("write_error", Json::text(e)),
            };
            (Json::Number((*offset).into()), Json::text(kind), detail)
        }
    };
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("genuine", Json::Bool(r.genuine())),
        ("claimed_bytes", Json::Number(r.claimed.into())),
        ("proven_bytes", Json::Number(r.proven_bytes.into())),
        ("probes", Json::Number(r.probes.into())),
        ("faults", Json::Number(r.faults.into())),
        ("first_fault_offset", offset),
        ("first_fault", kind),
        ("first_fault_detail", detail),
        ("restored", Json::Bool(r.restored)),
    ])
}

fn bad_text(bad: &Bad) -> String {
    match bad {
        Bad::WriteError(e) => format!("the write failed: {e}"),
        Bad::ReadError(e) => format!("the read failed: {e}"),
        Bad::Mismatch => "it reads back as other data".to_string(),
    }
}

pub fn verify_text(drive: &DriveInfo, r: &VerifyReport) -> Vec<String> {
    let mut lines = vec![heading("Write test", drive), String::new()];
    lines.push(format!(
        "Written:   {} in {}, at {}",
        human_size(r.bytes_written),
        duration(r.write_micros),
        speed(r.write_bytes_per_second())
    ));
    lines.push(format!(
        "Read:      {} in {}, at {}",
        human_size(r.bytes_read),
        duration(r.read_micros),
        speed(r.read_bytes_per_second())
    ));
    lines.push(format!(
        "Bad:       {} of {} blocks",
        grouped(r.bad_blocks),
        grouped(r.blocks)
    ));
    for (offset, bad) in r.bad.iter().take(SHOWN) {
        lines.push(format!("  at byte {}: {}", grouped(*offset), bad_text(bad)));
    }
    lines.push(String::new());
    lines.push(match (r.complete, r.bad_blocks) {
        (false, _) => "The test stopped before the end.".to_string(),
        (true, 0) => "Each block holds what the test wrote.".to_string(),
        (true, n) => format!("{n} blocks do not hold what the test wrote."),
    });
    lines.push("The drive now holds the test pattern. Format it before you use it.".to_string());
    lines
}

pub fn verify_json(drive: &DriveInfo, r: &VerifyReport) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("complete", Json::Bool(r.complete)),
        ("blocks", Json::Number(r.blocks.into())),
        ("bad_blocks", Json::Number(r.bad_blocks.into())),
        (
            "bad",
            Json::List(
                r.bad
                    .iter()
                    .map(|(offset, bad)| {
                        Json::Object(vec![
                            ("offset", Json::Number((*offset).into())),
                            ("why", Json::text(bad_text(bad))),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("bytes_written", Json::Number(r.bytes_written.into())),
        ("bytes_read", Json::Number(r.bytes_read.into())),
        (
            "write_bytes_per_second",
            Json::Number(r.write_bytes_per_second().into()),
        ),
        (
            "read_bytes_per_second",
            Json::Number(r.read_bytes_per_second().into()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_bench::bench::JOBS;
    use onyx_core::DriveId;

    fn drive() -> DriveInfo {
        let mut d = DriveInfo::new(DriveId::new("disk4"), "/dev/rdisk4", "VendorCo ProductCode");
        d.size_bytes = 125_829_120_000;
        d
    }

    #[test]
    fn a_duration_reads_in_seconds_minutes_or_hours() {
        assert_eq!(duration(93_000), "93 ms");
        assert_eq!(duration(1_500_000), "1.5 s");
        assert_eq!(duration(125_000_000), "2 min 5 s");
        assert_eq!(duration(7_384_000_000), "2 h 3 min");
    }

    #[test]
    fn a_scan_prints_each_class_and_its_verdict() {
        let r = ScanReport {
            block_bytes: scan::BLOCK_BYTES,
            classes: [900, 90, 9, 1, 0, 0],
            failed_blocks: 0,
            slow: vec![(131_072, 250_000)],
            bytes_read: 1000 * scan::BLOCK_BYTES,
            micros: 10_000_000,
            complete: true,
            ..ScanReport::default()
        };
        let lines = scan_text(&drive(), &r);
        assert_eq!(
            lines[0],
            "Surface scan of VendorCo ProductCode (disk4), 125.8 GB"
        );
        assert_eq!(lines[2], "< 5 ms      900");
        assert_eq!(lines[7], ">= 600 ms   0");
        assert_eq!(lines[8], "failed      0");
        assert!(lines.iter().any(|l| l == "  slow at byte 131,072: 250 ms"));
        assert_eq!(
            lines.last().unwrap(),
            "Each block read. 1 blocks took 200 ms or more."
        );
        let json = scan_json(&drive(), &r).to_string();
        assert!(
            json.contains(r#""classes":[{"class":"< 5 ms","blocks":900}"#),
            "{json}"
        );
    }

    #[test]
    fn a_scan_with_a_failed_block_says_that_the_drive_loses_data() {
        let r = ScanReport {
            failed_blocks: 2,
            failed: vec![(0, "Input/output error".to_string())],
            complete: true,
            ..ScanReport::default()
        };
        let lines = scan_text(&drive(), &r);
        assert!(
            lines
                .iter()
                .any(|l| l == "  failed at byte 0: Input/output error")
        );
        assert!(lines.last().unwrap().contains("losing data"));
    }

    fn result(job: onyx_bench::bench::Job, mb: u64) -> JobResult {
        JobResult {
            job,
            bytes: mb * 1_000_000,
            commands: mb,
            micros: 1_000_000,
            busy_micros: mb * 1000,
        }
    }

    #[test]
    fn the_bench_table_lines_up_its_numbers_on_the_right() {
        let reads = [result(JOBS[0], 2000), result(JOBS[3], 45)];
        let writes = [
            result(JOBS[0].writing(), 1500),
            result(JOBS[3].writing(), 120),
        ];
        let lines = bench_text("Benchmark", &reads, &writes);
        assert!(lines[2].starts_with("JOB") && lines[2].ends_with("LATENCY"));
        assert!(lines[3].starts_with("SEQ1M Q8T1"));
        assert!(lines[3].contains("2.0 GB/s") && lines[3].contains("1.5 GB/s"));
        assert_eq!(lines[3].len(), lines[4].len(), "{lines:#?}");
        let json = bench_json("disk4", &reads, &writes).to_string();
        assert!(
            json.contains(r#"{"job":"SEQ1M Q8T1","write":true,"bytes_per_second":1500000000"#),
            "{json}"
        );
    }

    #[test]
    fn a_read_only_bench_has_no_write_columns() {
        let lines = bench_text("Benchmark", &[result(JOBS[1], 30)], &[]);
        assert!(!lines[2].contains("WRITE"));
    }

    #[test]
    fn a_fake_drive_is_named_and_its_real_size_bracketed() {
        let r = CapacityReport {
            claimed: 128_000_000_000,
            probes: 1024,
            first_fault: Some((32_031_000_000, Fault::Wraps { onto: 31_000_000 })),
            faults: 768,
            proven_bytes: 31_906_000_000,
            restored: true,
        };
        let lines = capacity_text(&drive(), &r);
        assert!(
            lines
                .iter()
                .any(|l| l
                    .starts_with("FAKE. The drive holds at least 31.9 GB and less than 32.0 GB")),
            "{lines:#?}"
        );
        assert!(lines.iter().any(|l| l.contains("so the drive wraps")));
        let json = capacity_json(&drive(), &r).to_string();
        assert!(json.contains(r#""genuine":false"#));
        assert!(
            json.contains(r#""first_fault":"wraps","first_fault_detail":31000000"#),
            "{json}"
        );
    }

    #[test]
    fn a_genuine_drive_says_so() {
        let r = CapacityReport {
            claimed: 128_000_000_000,
            probes: 1024,
            first_fault: None,
            faults: 0,
            proven_bytes: 128_000_000_000,
            restored: true,
        };
        let lines = capacity_text(&drive(), &r);
        assert!(lines.last().unwrap().starts_with("Genuine."));
        assert!(
            capacity_json(&drive(), &r)
                .to_string()
                .contains(r#""first_fault":null"#)
        );
    }

    #[test]
    fn a_write_test_says_that_the_drive_holds_the_pattern_now() {
        let r = VerifyReport {
            blocks: 10,
            bad_blocks: 1,
            bad: vec![(1_048_576, Bad::Mismatch)],
            complete: true,
            ..VerifyReport::default()
        };
        let lines = verify_text(&drive(), &r);
        assert!(lines.iter().any(|l| l == "Bad:       1 of 10 blocks"));
        assert!(
            lines
                .iter()
                .any(|l| l == "  at byte 1,048,576: it reads back as other data")
        );
        assert!(lines.last().unwrap().contains("Format it"));
    }
}
