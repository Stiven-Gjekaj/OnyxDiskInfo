//! The write test: write every block of a drive, then read each one back.
//!
//! **This erases the drive.** It writes a pattern over every byte, and it
//! puts nothing back. The command line asks the person to confirm the drive
//! first.
//!
//! The pattern of each block comes from its offset and from the run, so a
//! block that the drive writes to the wrong place reads back wrong, and the
//! test keeps no copy of the drive in memory.

use std::time::Instant;

use crate::capacity::fill;
use crate::device::{AlignedBuffer, Device};

/// The size of one block of the write test: 1 MiB.
pub const BLOCK_BYTES: u64 = 1024 * 1024;

/// The most bad blocks that a report lists one by one.
pub const LIST_LIMIT: usize = 1000;

/// The two passes of the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Write,
    Read,
}

/// The progress of the test, for a person who waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub pass: Pass,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub bad_blocks: u64,
}

/// Why one block is bad.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bad {
    WriteError(String),
    ReadError(String),
    /// The block read back, and it is not what the test wrote.
    Mismatch,
}

/// The result of the write test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub block_bytes: u64,
    pub blocks: u64,
    pub bad_blocks: u64,
    /// The offset of each bad block and why, up to [`LIST_LIMIT`].
    pub bad: Vec<(u64, Bad)>,
    /// The time of each pass, in microseconds.
    pub write_micros: u64,
    pub read_micros: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
    /// The test reached the end of both passes.
    pub complete: bool,
}

impl VerifyReport {
    fn add(&mut self, offset: u64, bad: Bad) {
        self.bad_blocks += 1;
        if self.bad.len() < LIST_LIMIT {
            self.bad.push((offset, bad));
        }
    }

    /// The write speed, in bytes per second.
    pub fn write_bytes_per_second(&self) -> u64 {
        (u128::from(self.bytes_written) * 1_000_000 / u128::from(self.write_micros.max(1))) as u64
    }

    /// The read speed, in bytes per second.
    pub fn read_bytes_per_second(&self) -> u64 {
        (u128::from(self.bytes_read) * 1_000_000 / u128::from(self.read_micros.max(1))) as u64
    }
}

/// The blocks of a device: each offset and length, in whole sectors.
pub(crate) fn blocks(device: &dyn Device) -> Vec<(u64, u64)> {
    let total = device.size();
    let sector = u64::from(device.sector_size().max(1));
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < total {
        let length = (total - offset).min(BLOCK_BYTES) / sector * sector;
        if length == 0 {
            break;
        }
        out.push((offset, length));
        offset += length;
    }
    out
}

/// Write every block, flush, then read each one back and compare.
///
/// A bad block does not stop the test. `progress` hears about each block,
/// and the test stops early when it gives back `false`.
pub fn test(
    device: &dyn Device,
    run: u64,
    mut progress: impl FnMut(Progress) -> bool,
) -> VerifyReport {
    let plan = blocks(device);
    let total = device.size();
    let mut report = VerifyReport {
        block_bytes: BLOCK_BYTES,
        blocks: plan.len() as u64,
        ..VerifyReport::default()
    };
    let mut buffer = AlignedBuffer::new(BLOCK_BYTES as usize);
    let mut expected = vec![0u8; BLOCK_BYTES as usize];
    let mut write_failed = vec![false; plan.len()];

    let began = Instant::now();
    for (index, (offset, length)) in plan.iter().enumerate() {
        let bytes = &mut buffer.bytes_mut()[..*length as usize];
        fill(bytes, run, *offset);
        match device.write_at(bytes, *offset) {
            Ok(()) => report.bytes_written += length,
            Err(e) => {
                write_failed[index] = true;
                report.add(*offset, Bad::WriteError(e.to_string()));
            }
        }
        let go_on = progress(Progress {
            pass: Pass::Write,
            done_bytes: offset + length,
            total_bytes: total,
            bad_blocks: report.bad_blocks,
        });
        if !go_on {
            report.write_micros = began.elapsed().as_micros() as u64;
            return report;
        }
    }
    // The flush is part of the write pass. A drive that keeps the writes in
    // a cache has not written them until the flush returns.
    if let Err(e) = device.flush() {
        report.add(0, Bad::WriteError(format!("the flush failed: {e}")));
    }
    report.write_micros = began.elapsed().as_micros() as u64;

    let began = Instant::now();
    for (index, (offset, length)) in plan.iter().enumerate() {
        if !write_failed[index] {
            let want = &mut expected[..*length as usize];
            fill(want, run, *offset);
            let bytes = &mut buffer.bytes_mut()[..*length as usize];
            match device.read_at(bytes, *offset) {
                Ok(()) if bytes == want => report.bytes_read += length,
                Ok(()) => {
                    report.bytes_read += length;
                    report.add(*offset, Bad::Mismatch);
                }
                Err(e) => report.add(*offset, Bad::ReadError(e.to_string())),
            }
        }
        let go_on = progress(Progress {
            pass: Pass::Read,
            done_bytes: offset + length,
            total_bytes: total,
            bad_blocks: report.bad_blocks,
        });
        if !go_on {
            report.read_micros = began.elapsed().as_micros() as u64;
            return report;
        }
    }
    report.read_micros = began.elapsed().as_micros() as u64;
    report.complete = true;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;
    use std::io;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn a_healthy_device_has_no_bad_block_and_holds_the_pattern() {
        let d = MemoryDevice::new(5 * MIB + 4096, 512);
        let r = test(&d, 9, |_| true);
        assert!(r.complete);
        assert_eq!(r.blocks, 6);
        assert_eq!(r.bad_blocks, 0);
        assert_eq!(r.bytes_written, (5 * MIB + 4096) as u64);
        assert_eq!(r.bytes_read, r.bytes_written);
        // The drive now holds the pattern of the last block.
        let mut want = vec![0u8; 4096];
        fill(&mut want, 9, 5 * MIB as u64);
        assert_eq!(&d.contents()[5 * MIB..], want.as_slice());
    }

    /// A device that changes one byte in the block at `rot` when it reads it.
    struct Rotten {
        inner: MemoryDevice,
        rot: u64,
        unwritable: u64,
    }

    impl Device for Rotten {
        fn size(&self) -> u64 {
            self.inner.size()
        }
        fn sector_size(&self) -> u32 {
            512
        }
        fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
            self.inner.read_at(buffer, offset)?;
            if offset == self.rot {
                buffer[100] ^= 1;
            }
            Ok(())
        }
        fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
            if offset == self.unwritable {
                return Err(io::Error::other("write fault"));
            }
            self.inner.write_at(buffer, offset)
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_changed_byte_and_a_failed_write_are_each_a_bad_block() {
        let d = Rotten {
            inner: MemoryDevice::new(4 * MIB, 512),
            rot: 2 * MIB as u64,
            unwritable: MIB as u64,
        };
        let r = test(&d, 9, |_| true);
        assert!(r.complete);
        assert_eq!(r.bad_blocks, 2);
        assert_eq!(
            r.bad[0],
            (MIB as u64, Bad::WriteError("write fault".to_string()))
        );
        assert_eq!(r.bad[1], (2 * MIB as u64, Bad::Mismatch));
    }

    #[test]
    fn the_person_can_stop_the_test_in_either_pass() {
        let d = MemoryDevice::new(4 * MIB, 512);
        let r = test(&d, 9, |p| {
            p.pass == Pass::Write && p.done_bytes < 2 * MIB as u64
        });
        assert!(!r.complete);
        assert_eq!(r.bytes_written, 2 * MIB as u64);
        assert_eq!(r.bytes_read, 0);
        let r = test(&d, 9, |p| {
            p.pass == Pass::Write || p.done_bytes < MIB as u64
        });
        assert!(!r.complete);
        assert_eq!(r.bytes_read, MIB as u64);
    }

    #[test]
    fn two_runs_write_different_patterns() {
        let d = MemoryDevice::new(MIB, 512);
        test(&d, 1, |_| true);
        let first = d.contents();
        test(&d, 2, |_| true);
        assert_ne!(first, d.contents());
    }
}
