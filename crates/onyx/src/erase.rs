//! `onyx erase`: write over every byte of a drive, or TRIM it.

use onyx_bench::Device;
use onyx_bench::erase::{self, EraseReport, Fill};
use onyx_core::{DriveInfo, Result, human_size};

use crate::cli::{PassChoice, WriteTarget};
use crate::disk_tests::{Meter, device, open_for_write, run_mark};
use crate::json::Json;

/// The blocks that the check after a TRIM reads, spread over the drive.
pub const TRIM_SAMPLES: u64 = 64;

fn fill(choice: PassChoice) -> Fill {
    match choice {
        PassChoice::Zero => Fill::Zero,
        PassChoice::Ones => Fill::Ones,
        PassChoice::Random => Fill::Random,
    }
}

fn fill_name(fill: Fill) -> &'static str {
    match fill {
        Fill::Zero => "zero",
        Fill::Ones => "ones",
        Fill::Random => "random",
    }
}

/// What `onyx erase --json` prints after the passes.
pub fn overwrite_json(drive: &DriveInfo, r: &EraseReport) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("method", Json::text("overwrite")),
        (
            "passes",
            Json::List(r.passes.iter().map(|p| Json::text(fill_name(*p))).collect()),
        ),
        ("bytes_written", Json::Number(r.bytes_written.into())),
        ("checked", Json::Bool(r.checked)),
        ("bytes_checked", Json::Number(r.bytes_checked.into())),
        ("bad_blocks", Json::Number(r.bad_blocks.into())),
        (
            "bad",
            Json::List(
                r.bad
                    .iter()
                    .map(|(offset, why)| {
                        Json::Object(vec![
                            ("offset", Json::Number((*offset).into())),
                            ("why", Json::text(why)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("complete", Json::Bool(r.complete)),
        ("micros", Json::Number(r.micros.into())),
    ])
}

/// What `onyx erase` prints for a person after the passes.
pub fn overwrite_text(drive: &DriveInfo, r: &EraseReport) -> Vec<String> {
    let names: Vec<&str> = r.passes.iter().map(|p| fill_name(*p)).collect();
    let mut out = vec![format!(
        "Erase of {} ({}): {} pass{} ({}), {} written",
        drive.name,
        drive.id,
        r.passes.len(),
        if r.passes.len() == 1 { "" } else { "es" },
        names.join(", "),
        human_size(r.bytes_written),
    )];
    if r.checked {
        out.push(format!(
            "The last pass reads back: {} checked, {} bad blocks.",
            human_size(r.bytes_checked),
            r.bad_blocks
        ));
    } else {
        out.push(format!(
            "The last pass was not read back. {} bad blocks.",
            r.bad_blocks
        ));
    }
    for (offset, why) in r.bad.iter().take(20) {
        out.push(format!("  at byte {offset}: {why}"));
    }
    if !r.complete {
        out.push("The erase stopped before the end.".to_string());
    }
    out
}

/// `onyx erase`: write each pass over the drive.
pub fn run_overwrite(target: &WriteTarget, choices: &[PassChoice], check: bool) -> Result<i32> {
    let passes: Vec<Fill> = if choices.is_empty() {
        vec![Fill::Zero]
    } else {
        choices.iter().map(|c| fill(*c)).collect()
    };
    let opened = open_for_write(target, "The erase writes over every byte of the drive. It")?;
    let drive = &opened.drive;
    let device = device(drive, &opened.file)?;
    let mut meter = Meter::new();
    let report = erase::overwrite(&device, &passes, check, run_mark(), |p| {
        let what = if p.checking {
            "Reading back".to_string()
        } else {
            format!("Pass {} of {}", p.pass, p.passes)
        };
        meter.show(&what, p.done_bytes, p.total_bytes, p.bad_blocks);
        true
    });
    meter.done();
    if target.json {
        println!("{}", overwrite_json(drive, &report));
    } else {
        for line in overwrite_text(drive, &report) {
            println!("{line}");
        }
    }
    Ok(if report.bad_blocks > 0 || !report.complete {
        2
    } else {
        0
    })
}

/// Read blocks spread over the drive after a TRIM, and count the ones that
/// hold only zeros.
fn zero_samples(device: &dyn Device) -> Result<u64> {
    let block = 1024 * 1024u64;
    let sector = u64::from(device.sector_size().max(1));
    let span = device.size() / sector * sector;
    let mut buffer = onyx_bench::AlignedBuffer::new(block as usize);
    let mut zeros = 0;
    for i in 0..TRIM_SAMPLES {
        let offset = (span.saturating_sub(block) * i / (TRIM_SAMPLES - 1).max(1)) / sector * sector;
        let length = block.min(span - offset) as usize;
        let bytes = &mut buffer.bytes_mut()[..length];
        device.read_at(bytes, offset)?;
        if bytes.iter().all(|b| *b == 0) {
            zeros += 1;
        }
    }
    Ok(zeros)
}

/// What a TRIM found, for a person.
pub fn trim_text(drive: &DriveInfo, zeros: u64) -> Vec<String> {
    let mut out = vec![format!(
        "TRIM of {} ({}): the drive took a TRIM of all {}.",
        drive.name,
        drive.id,
        human_size(drive.size_bytes)
    )];
    out.push(format!(
        "{zeros} of {TRIM_SAMPLES} blocks spread over the drive read back as zeros."
    ));
    if zeros < TRIM_SAMPLES {
        out.push(
            "The drive still gives old bytes from some blocks. Some drives erase a trimmed \
             block later. Run an overwrite pass if no old byte may stay readable."
                .to_string(),
        );
    }
    out
}

/// What `onyx erase --trim --json` prints.
pub fn trim_json(drive: &DriveInfo, zeros: u64) -> Json {
    Json::Object(vec![
        ("id", Json::text(drive.id.as_str())),
        ("method", Json::text("trim")),
        ("bytes", Json::Number(drive.size_bytes.into())),
        ("samples", Json::Number(TRIM_SAMPLES.into())),
        ("zero_samples", Json::Number(zeros.into())),
    ])
}

/// `onyx erase --trim`: TRIM the whole drive, then read a sample.
pub fn run_trim(target: &WriteTarget) -> Result<i32> {
    let opened = open_for_write(
        target,
        "The TRIM tells the drive that no block holds data. It",
    )?;
    let drive = &opened.drive;
    let mut meter = Meter::new();
    onyx_probe::trim::trim(
        &opened.file,
        drive.size_bytes,
        drive.logical_sector_size,
        |done| meter.show("TRIM", done, drive.size_bytes, 0),
    )?;
    meter.done();
    let zeros = zero_samples(&device(drive, &opened.file)?)?;
    if target.json {
        println!("{}", trim_json(drive, zeros));
    } else {
        for line in trim_text(drive, zeros) {
            println!("{line}");
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_bench::MemoryDevice;
    use onyx_core::DriveId;

    fn image(name: &str, size: u64) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("onyx-erase-{}-{name}.img", std::process::id()));
        std::fs::write(&path, vec![0xA5u8; size as usize]).unwrap();
        path
    }

    fn target(path: &std::path::Path, confirm: &str) -> WriteTarget {
        WriteTarget {
            drive: None,
            image: Some(path.to_path_buf()),
            force: false,
            confirm: Some(confirm.to_string()),
            json: true,
        }
    }

    #[test]
    fn an_image_holds_only_zeros_after_the_default_erase() {
        let size = 3 * 1024 * 1024;
        let path = image("zero", size);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let code = run_overwrite(&target(&path, &format!("{name} {size}")), &[], true).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(code, 0);
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn a_trim_of_an_image_file_is_refused_after_the_phrase() {
        let size = 1024 * 1024;
        let path = image("trim", size);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let result = run_trim(&target(&path, &format!("{name} {size}")));
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(result.is_err());
        assert!(bytes.iter().all(|b| *b == 0xA5));
    }

    #[test]
    fn the_sample_counts_the_blocks_of_zeros() {
        let d = MemoryDevice::new(8 * 1024 * 1024, 512);
        assert_eq!(zero_samples(&d).unwrap(), TRIM_SAMPLES);
        d.write_at(&[1u8; 512], 0).unwrap();
        assert_eq!(zero_samples(&d).unwrap(), TRIM_SAMPLES - 1);
    }

    #[test]
    fn the_text_says_when_old_bytes_stay() {
        let mut drive = DriveInfo::new(DriveId::new("sdz"), "/dev/sdz", "sdz");
        drive.size_bytes = 1 << 30;
        assert_eq!(trim_text(&drive, TRIM_SAMPLES).len(), 2);
        assert!(trim_text(&drive, 3)[2].contains("old bytes"));
        assert!(
            trim_json(&drive, 3)
                .to_string()
                .contains(r#""zero_samples":3"#)
        );
    }
}
