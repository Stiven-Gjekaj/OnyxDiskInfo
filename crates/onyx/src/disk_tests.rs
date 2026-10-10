//! The disk test commands: scan, bench, and the two tests that write.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use onyx_bench::bench::{self, JOBS, JobResult};
use onyx_bench::{Device, FileDevice, capacity, scan, verify};
use onyx_core::{
    DriveInfo, Error, Force, Result, by_index, check_same_drive, check_target, describe,
    force_phrase, human_size, in_list_order, phrase_matches,
};
use onyx_probe::{Access, OpenDrive, open_drive, open_test_file};

use crate::tests_format as format;

/// The drive with this number, from a fresh list.
fn find(number: usize) -> Result<DriveInfo> {
    let drives = in_list_order(onyx_probe::drive_list()?.drives()?);
    by_index(&drives, number)
        .cloned()
        .ok_or_else(|| Error::NoSuchDrive {
            wanted: number.to_string(),
        })
}

/// A device on a drive that is open.
fn device(drive: &DriveInfo, open: &OpenDrive) -> Result<FileDevice> {
    Ok(FileDevice::new(
        open.file.try_clone()?,
        drive.size_bytes,
        drive.logical_sector_size,
    ))
}

/// A value that changes from run to run, to mark the blocks of one run.
fn run_mark() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5eed)
}

/// A line of progress on the error stream, at most twice a second, so that
/// the output of the command stays clean for a script.
struct Meter {
    last: Instant,
    shown: bool,
}

impl Meter {
    fn new() -> Self {
        Meter {
            last: Instant::now(),
            shown: false,
        }
    }

    fn show(&mut self, what: &str, done: u64, total: u64, bad: u64) {
        if self.last.elapsed() < Duration::from_millis(500) && done < total {
            return;
        }
        self.last = Instant::now();
        self.shown = true;
        let percent = done as f64 * 100.0 / total.max(1) as f64;
        eprint!(
            "\r{what}: {percent:5.1}% of {}, {bad} bad   ",
            human_size(total)
        );
        let _ = io::stderr().flush();
    }

    fn done(&self) {
        if self.shown {
            eprintln!();
        }
    }
}

/// `onyx scan`: read every block of a drive.
pub fn run_scan(number: usize, json: bool) -> Result<i32> {
    let drive = find(number)?;
    let open = open_drive(&drive, Access::Read)?;
    let device = device(&drive, &open)?;
    let mut meter = Meter::new();
    let report = scan::scan(&device, |p| {
        meter.show("Reading", p.done_bytes, p.total_bytes, p.failed_blocks);
        true
    });
    meter.done();
    if json {
        println!("{}", format::scan_json(&drive, &report));
    } else {
        for line in format::scan_text(&drive, &report) {
            println!("{line}");
        }
    }
    Ok(if report.failed_blocks > 0 { 2 } else { 0 })
}

/// Run each job of CrystalDiskMark, for reading and, when `write` is true,
/// for writing.
fn run_jobs(
    device: &dyn Device,
    span: u64,
    seconds: u64,
    write: bool,
) -> Result<(Vec<JobResult>, Vec<JobResult>)> {
    let duration = Duration::from_secs(seconds);
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    for job in JOBS {
        eprintln!("{}: reading", job.name);
        reads.push(bench::run(device, job, 0, span, duration, run_mark())?);
        if write {
            eprintln!("{}: writing", job.name);
            writes.push(bench::run(
                device,
                job.writing(),
                0,
                span,
                duration,
                run_mark(),
            )?);
        }
    }
    Ok((reads, writes))
}

/// `onyx bench <number>`: read jobs on a drive. Nothing is written.
pub fn run_bench_drive(number: usize, seconds: u64, json: bool) -> Result<i32> {
    let drive = find(number)?;
    let open = open_drive(&drive, Access::Read)?;
    let device = device(&drive, &open)?;
    let span = bench::SPAN_BYTES.min(drive.size_bytes);
    let (reads, _) = run_jobs(&device, span, seconds, false)?;
    if json {
        println!("{}", format::bench_json(drive.id.as_str(), &reads, &[]));
    } else {
        let what = format!(
            "Benchmark of {} ({}): reads over the first {}, {seconds} s each",
            drive.name,
            drive.id,
            human_size(span)
        );
        for line in format::bench_text(&what, &reads, &[]) {
            println!("{line}");
        }
    }
    Ok(0)
}

/// Fill a new test file, so that a read reads data the drive holds, and not
/// a hole that the file system makes up.
fn prepare(device: &dyn Device, span: u64) -> Result<()> {
    let block = 1024 * 1024;
    let mut buffer = onyx_bench::AlignedBuffer::new(block as usize);
    buffer.bytes_mut().fill(0x5a);
    let mut offset = 0;
    while offset < span {
        device.write_at(buffer.bytes(), offset)?;
        offset += block;
    }
    device.flush()?;
    Ok(())
}

/// `onyx bench --file <path>`: read and write jobs in a test file, which
/// goes at the end.
pub fn run_bench_file(path: &Path, size_mib: u64, seconds: u64, json: bool) -> Result<i32> {
    if path.exists() {
        return Err(Error::host(
            path.display().to_string(),
            "the file exists. Name a new file, so that no data is lost",
        ));
    }
    let span = size_mib * 1024 * 1024;
    let file = open_test_file(path)?;
    file.set_len(span)?;
    let device = FileDevice::new(file, span, 4096);
    eprintln!("Filling the test file of {}", human_size(span));
    let result = prepare(&device, span).and_then(|()| run_jobs(&device, span, seconds, true));
    drop(device);
    let removed = std::fs::remove_file(path);
    let (reads, writes) = result?;
    removed?;
    if json {
        println!(
            "{}",
            format::bench_json(&path.display().to_string(), &reads, &writes)
        );
    } else {
        let what = format!(
            "Benchmark in {}: a test file of {}, {seconds} s each",
            path.display(),
            human_size(span)
        );
        for line in format::bench_text(&what, &reads, &writes) {
            println!("{line}");
        }
    }
    Ok(0)
}

/// Check the drive, ask for the phrase, and open the drive for a test that
/// writes.
///
/// The phrase comes from `--confirm`, or from the person at the terminal.
/// A script that gives no phrase gets an error and not a prompt that waits.
fn open_for_write(
    number: usize,
    force: bool,
    confirm: Option<&str>,
    what: &str,
) -> Result<(DriveInfo, OpenDrive)> {
    let drive = find(number)?;
    let force = if force { Force::Yes } else { Force::No };
    check_target(&drive, force)?;

    let phrase = force_phrase(&drive);
    let typed = match confirm {
        Some(typed) => typed.to_string(),
        None => {
            if !io::stdin().is_terminal() {
                return Err(Error::NotConfirmed);
            }
            eprintln!("{what} writes to {}.", describe(&drive));
            eprintln!("Type this to go on: {phrase}");
            eprint!("> ");
            let _ = io::stderr().flush();
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            line
        }
    };
    if !phrase_matches(&typed, &drive) {
        return Err(Error::NotConfirmed);
    }

    // The list again, in case a drive came or went while the person typed.
    let again = find(number)?;
    check_same_drive(&drive, &again)?;
    let open = open_drive(&drive, Access::Write)?;
    Ok((drive, open))
}

/// `onyx test capacity`: does the drive hold what it claims?
pub fn run_capacity(number: usize, force: bool, confirm: Option<&str>, json: bool) -> Result<i32> {
    let (drive, open) = open_for_write(
        number,
        force,
        confirm,
        "The capacity test saves 64 MiB of the drive, writes over it, and puts it back. It",
    )?;
    let device = device(&drive, &open)?;
    eprintln!("Writing {} probes", capacity::PROBES);
    let report = capacity::test(&device, run_mark())?;
    if json {
        println!("{}", format::capacity_json(&drive, &report));
    } else {
        for line in format::capacity_text(&drive, &report) {
            println!("{line}");
        }
    }
    Ok(if report.genuine() { 0 } else { 2 })
}

/// `onyx test write`: write every block, and read each one back.
pub fn run_write(number: usize, force: bool, confirm: Option<&str>, json: bool) -> Result<i32> {
    let (drive, open) = open_for_write(
        number,
        force,
        confirm,
        "The write test erases every byte of the drive. It",
    )?;
    let device = device(&drive, &open)?;
    let mut meter = Meter::new();
    let report = verify::test(&device, run_mark(), |p| {
        let what = match p.pass {
            verify::Pass::Write => "Writing",
            verify::Pass::Read => "Reading",
        };
        meter.show(what, p.done_bytes, p.total_bytes, p.bad_blocks);
        true
    });
    meter.done();
    if json {
        println!("{}", format::verify_json(&drive, &report));
    } else {
        for line in format::verify_text(&drive, &report) {
            println!("{line}");
        }
    }
    Ok(if report.bad_blocks > 0 { 2 } else { 0 })
}
