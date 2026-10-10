//! The surface scan: read every block of a drive, and time each read.
//!
//! The scan only reads. It finds a block that the drive cannot read, and a
//! block that the drive reads only after it retries, which is slow. A block
//! that is slow today is often a block that fails later.
//!
//! The classes are the ones that MHDD and Victoria use, for blocks of 256
//! sectors of 512 bytes. A person who knows those tools reads the map the
//! same way.

use std::io;
use std::time::Instant;

use crate::device::{AlignedBuffer, Device};

/// The size of one block of the scan: 256 sectors of 512 bytes.
pub const BLOCK_BYTES: u64 = 128 * 1024;

/// The upper limits of the time classes, in milliseconds. A read at or above
/// the last limit is in the slowest class.
pub const LIMITS_MS: [u64; 5] = [5, 20, 50, 200, 600];

/// The number of time classes, and the class of a read that failed after
/// them.
pub const CLASSES: usize = LIMITS_MS.len() + 1;

/// The most failed blocks and slow blocks that a report lists one by one.
/// The counts include every block.
pub const LIST_LIMIT: usize = 1000;

/// The time class of a read that took `micros`.
pub fn class_of(micros: u64) -> usize {
    LIMITS_MS
        .iter()
        .position(|limit| micros < limit * 1000)
        .unwrap_or(LIMITS_MS.len())
}

/// A read that took at least this long goes on the list of slow blocks.
pub const SLOW_MS: u64 = 200;

/// The progress of a scan, for a person who waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub failed_blocks: u64,
}

/// What a scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub block_bytes: u64,
    /// The number of blocks in each time class, fastest first.
    pub classes: [u64; CLASSES],
    pub failed_blocks: u64,
    /// The offset and the message of each failed block, up to
    /// [`LIST_LIMIT`].
    pub failed: Vec<(u64, String)>,
    /// The offset and the time of each slow block, up to [`LIST_LIMIT`].
    pub slow: Vec<(u64, u64)>,
    pub bytes_read: u64,
    /// The time of the whole scan, in microseconds.
    pub micros: u64,
    /// The scan reached the end of the drive. False when the person stopped
    /// it.
    pub complete: bool,
}

impl ScanReport {
    /// The number of blocks that the scan read or tried to read.
    pub fn blocks(&self) -> u64 {
        self.classes.iter().sum::<u64>() + self.failed_blocks
    }

    /// The mean speed of the reads that worked, in bytes per second.
    pub fn bytes_per_second(&self) -> u64 {
        (u128::from(self.bytes_read) * 1_000_000 / u128::from(self.micros.max(1))) as u64
    }
}

/// Read every block of `device`, from the first to the last.
///
/// A failed read does not stop the scan. `progress` hears about each block,
/// and the scan stops early when it gives back `false`.
pub fn scan(device: &dyn Device, mut progress: impl FnMut(Progress) -> bool) -> ScanReport {
    let total = device.size();
    let sector = u64::from(device.sector_size().max(1));
    let mut report = ScanReport {
        block_bytes: BLOCK_BYTES,
        complete: true,
        ..ScanReport::default()
    };
    let mut buffer = AlignedBuffer::new(BLOCK_BYTES as usize);
    let began = Instant::now();

    let mut offset = 0u64;
    while offset < total {
        // The last block is what is left, in whole sectors.
        let length = (total - offset).min(BLOCK_BYTES) / sector * sector;
        if length == 0 {
            break;
        }
        let bytes = &mut buffer.bytes_mut()[..length as usize];
        let start = Instant::now();
        let result = device.read_at(bytes, offset);
        let micros = start.elapsed().as_micros() as u64;
        match result {
            Ok(()) => record_read(&mut report, offset, length, micros),
            Err(e) => record_failure(&mut report, offset, &e),
        }
        offset += length;

        let keep_going = progress(Progress {
            done_bytes: offset,
            total_bytes: total,
            failed_blocks: report.failed_blocks,
        });
        if !keep_going {
            report.complete = offset >= total;
            break;
        }
    }
    report.micros = began.elapsed().as_micros() as u64;
    report
}

fn record_read(report: &mut ScanReport, offset: u64, length: u64, micros: u64) {
    report.classes[class_of(micros)] += 1;
    report.bytes_read += length;
    if micros >= SLOW_MS * 1000 && report.slow.len() < LIST_LIMIT {
        report.slow.push((offset, micros));
    }
}

fn record_failure(report: &mut ScanReport, offset: u64, error: &io::Error) {
    report.failed_blocks += 1;
    if report.failed.len() < LIST_LIMIT {
        report.failed.push((offset, error.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;

    /// A device that fails each read that touches one of its bad offsets.
    struct Damaged {
        inner: MemoryDevice,
        bad: Vec<u64>,
    }

    impl Device for Damaged {
        fn size(&self) -> u64 {
            self.inner.size()
        }
        fn sector_size(&self) -> u32 {
            self.inner.sector_size()
        }
        fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
            let end = offset + buffer.len() as u64;
            if self.bad.iter().any(|b| (offset..end).contains(b)) {
                return Err(io::Error::other("Input/output error"));
            }
            self.inner.read_at(buffer, offset)
        }
        fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
            self.inner.write_at(buffer, offset)
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn each_time_falls_in_the_class_of_mhdd() {
        assert_eq!(class_of(0), 0);
        assert_eq!(class_of(4_999), 0);
        assert_eq!(class_of(5_000), 1);
        assert_eq!(class_of(199_999), 3);
        assert_eq!(class_of(599_999), 4);
        assert_eq!(class_of(600_000), 5);
        assert_eq!(class_of(u64::MAX), 5);
    }

    #[test]
    fn a_healthy_device_is_read_to_the_end_with_no_failure() {
        let d = MemoryDevice::new(10 * BLOCK_BYTES as usize, 512);
        let r = scan(&d, |_| true);
        assert!(r.complete);
        assert_eq!(r.blocks(), 10);
        assert_eq!(r.failed_blocks, 0);
        assert_eq!(r.bytes_read, 10 * BLOCK_BYTES);
    }

    #[test]
    fn the_last_block_is_what_is_left_of_the_device() {
        let size = 3 * BLOCK_BYTES as usize + 4096;
        let r = scan(&MemoryDevice::new(size, 512), |_| true);
        assert_eq!(r.blocks(), 4);
        assert_eq!(r.bytes_read, size as u64);
    }

    #[test]
    fn a_failed_read_is_listed_and_the_scan_goes_on() {
        let d = Damaged {
            inner: MemoryDevice::new(8 * BLOCK_BYTES as usize, 512),
            bad: vec![2 * BLOCK_BYTES + 100, 5 * BLOCK_BYTES],
        };
        let r = scan(&d, |_| true);
        assert!(r.complete);
        assert_eq!(r.failed_blocks, 2);
        assert_eq!(r.failed[0].0, 2 * BLOCK_BYTES);
        assert_eq!(r.failed[1].0, 5 * BLOCK_BYTES);
        assert!(r.failed[0].1.contains("Input/output"));
        assert_eq!(r.blocks(), 8);
        assert_eq!(r.bytes_read, 6 * BLOCK_BYTES);
    }

    #[test]
    fn the_person_can_stop_the_scan() {
        let d = MemoryDevice::new(10 * BLOCK_BYTES as usize, 512);
        let mut calls = 0;
        let r = scan(&d, |p| {
            calls += 1;
            p.done_bytes < 3 * BLOCK_BYTES
        });
        assert!(!r.complete);
        assert_eq!(calls, 3);
        assert_eq!(r.blocks(), 3);
    }

    #[test]
    fn the_progress_counts_up_to_the_size() {
        let d = MemoryDevice::new(4 * BLOCK_BYTES as usize, 512);
        let mut seen = Vec::new();
        scan(&d, |p| {
            seen.push(p.done_bytes);
            true
        });
        assert_eq!(seen, [1, 2, 3, 4].map(|n| n * BLOCK_BYTES).to_vec());
    }

    #[test]
    fn a_device_smaller_than_a_sector_gives_an_empty_report() {
        let r = scan(&MemoryDevice::new(100, 512), |_| true);
        assert_eq!(r.blocks(), 0);
        assert!(r.complete);
    }
}
