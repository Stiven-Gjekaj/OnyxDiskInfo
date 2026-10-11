//! Erase a drive by writing over each byte, one pass or more.
//!
//! **This erases the drive**, and nothing puts it back. One pass of zeros
//! is the "clear" of NIST SP 800-88 for a disk that a person cannot read
//! past its own interface. A flash drive keeps spare blocks that no write
//! reaches, so the command line says that a firmware erase is stronger.
//!
//! The last pass reads back unless the person asks for no check. A block
//! that reads back wrong, or that fails, goes in the report, and the erase
//! goes on.

use std::time::Instant;

use crate::device::{AlignedBuffer, Device};
use crate::verify::{BLOCK_BYTES, blocks};

/// What one pass writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Zero,
    Ones,
    /// Bytes from a generator, with a seed of its own for each pass, so that
    /// the check can make the same bytes again.
    Random,
}

/// The most blocks that a report lists one by one.
pub const LIST_LIMIT: usize = 1000;

/// The progress of an erase, for a person who waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The pass, from 1. The check is the pass after the last one.
    pub pass: usize,
    pub passes: usize,
    pub checking: bool,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub bad_blocks: u64,
}

/// The result of an erase.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EraseReport {
    pub passes: Vec<Fill>,
    pub bytes_written: u64,
    pub bytes_checked: u64,
    pub micros: u64,
    /// The last pass was read back.
    pub checked: bool,
    pub bad_blocks: u64,
    /// The offset of each bad block and why, up to [`LIST_LIMIT`].
    pub bad: Vec<(u64, String)>,
    /// Each pass, and the check, reached the end.
    pub complete: bool,
}

impl EraseReport {
    fn add(&mut self, offset: u64, why: String) {
        self.bad_blocks += 1;
        if self.bad.len() < LIST_LIMIT {
            self.bad.push((offset, why));
        }
    }
}

/// The bytes of a pass for the block at `offset`.
fn fill(block: &mut [u8], fill: Fill, seed: u64, offset: u64) {
    match fill {
        Fill::Zero => block.fill(0),
        Fill::Ones => block.fill(0xFF),
        Fill::Random => {
            let mut state = seed ^ offset.rotate_left(29) ^ 0x2545_f491_4f6c_dd1d;
            for chunk in block.chunks_mut(8) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                chunk.copy_from_slice(&state.to_le_bytes()[..chunk.len()]);
            }
        }
    }
}

/// Write each pass over the whole device, then read the last one back when
/// `check` is true. `progress` hears about each block, and the erase stops
/// early when it gives back `false`.
pub fn overwrite(
    device: &dyn Device,
    passes: &[Fill],
    check: bool,
    seed: u64,
    mut progress: impl FnMut(Progress) -> bool,
) -> EraseReport {
    let plan = blocks(device);
    let total = device.size();
    let mut report = EraseReport {
        passes: passes.to_vec(),
        checked: check,
        ..EraseReport::default()
    };
    let mut buffer = AlignedBuffer::new(BLOCK_BYTES as usize);
    let mut expected = vec![0u8; BLOCK_BYTES as usize];
    let began = Instant::now();
    let seed_of = |pass: usize| {
        seed.wrapping_add(pass as u64)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
    };

    for (index, kind) in passes.iter().enumerate() {
        for (offset, length) in &plan {
            let bytes = &mut buffer.bytes_mut()[..*length as usize];
            fill(bytes, *kind, seed_of(index), *offset);
            match device.write_at(bytes, *offset) {
                Ok(()) => report.bytes_written += length,
                Err(e) => report.add(*offset, format!("write: {e}")),
            }
            let go_on = progress(Progress {
                pass: index + 1,
                passes: passes.len(),
                checking: false,
                done_bytes: offset + length,
                total_bytes: total,
                bad_blocks: report.bad_blocks,
            });
            if !go_on {
                report.micros = began.elapsed().as_micros() as u64;
                return report;
            }
        }
        if let Err(e) = device.flush() {
            report.add(0, format!("flush: {e}"));
        }
    }

    if let (true, Some(last)) = (check, passes.last()) {
        let index = passes.len() - 1;
        for (offset, length) in &plan {
            let length = *length as usize;
            let bytes = &mut buffer.bytes_mut()[..length];
            match device.read_at(bytes, *offset) {
                Ok(()) => {
                    fill(&mut expected[..length], *last, seed_of(index), *offset);
                    if bytes != &expected[..length] {
                        report.add(*offset, "reads back other bytes".to_string());
                    }
                    report.bytes_checked += length as u64;
                }
                Err(e) => report.add(*offset, format!("read: {e}")),
            }
            let go_on = progress(Progress {
                pass: passes.len() + 1,
                passes: passes.len(),
                checking: true,
                done_bytes: offset + length as u64,
                total_bytes: total,
                bad_blocks: report.bad_blocks,
            });
            if !go_on {
                report.micros = began.elapsed().as_micros() as u64;
                return report;
            }
        }
    }
    report.micros = began.elapsed().as_micros() as u64;
    report.complete = true;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;
    use std::io;

    const MIB: usize = 1024 * 1024;

    fn dirty(size: usize) -> MemoryDevice {
        let d = MemoryDevice::new(size, 512);
        d.write_at(&vec![0xA5; size], 0).unwrap();
        d
    }

    #[test]
    fn one_pass_of_zeros_leaves_only_zeros_and_checks_them() {
        // A size that is not a whole block: the last block is short.
        let d = dirty(3 * MIB + 512);
        let r = overwrite(&d, &[Fill::Zero], true, 1, |_| true);
        assert!(r.complete && r.checked);
        assert_eq!(r.bad_blocks, 0);
        assert_eq!(r.bytes_written, (3 * MIB + 512) as u64);
        assert_eq!(r.bytes_checked, r.bytes_written);
        assert!(d.contents().iter().all(|b| *b == 0));
    }

    #[test]
    fn the_last_pass_is_what_stays() {
        let d = dirty(2 * MIB);
        let r = overwrite(&d, &[Fill::Random, Fill::Ones], true, 9, |_| true);
        assert_eq!(r.bad_blocks, 0);
        assert!(d.contents().iter().all(|b| *b == 0xFF));
    }

    #[test]
    fn a_random_pass_checks_against_the_same_bytes_and_is_not_one_value() {
        let d = dirty(2 * MIB);
        let r = overwrite(&d, &[Fill::Random], true, 9, |_| true);
        assert_eq!(r.bad_blocks, 0);
        let c = d.contents();
        assert!(c.iter().filter(|b| **b == 0xA5).count() < c.len() / 100);
        // The two blocks differ: each offset has bytes of its own.
        assert_ne!(c[..MIB], c[MIB..]);
    }

    #[test]
    fn a_drive_that_drops_writes_fails_the_check() {
        struct Drops(MemoryDevice);
        impl Device for Drops {
            fn size(&self) -> u64 {
                self.0.size()
            }
            fn sector_size(&self) -> u32 {
                512
            }
            fn read_at(&self, b: &mut [u8], o: u64) -> io::Result<()> {
                self.0.read_at(b, o)
            }
            fn write_at(&self, b: &[u8], o: u64) -> io::Result<()> {
                // The second block is never written.
                if o == MIB as u64 {
                    Ok(())
                } else {
                    self.0.write_at(b, o)
                }
            }
            fn flush(&self) -> io::Result<()> {
                Ok(())
            }
        }
        let d = Drops(dirty(3 * MIB));
        let r = overwrite(&d, &[Fill::Zero], true, 1, |_| true);
        assert_eq!(r.bad_blocks, 1);
        assert_eq!(r.bad[0], (MIB as u64, "reads back other bytes".to_string()));
    }

    #[test]
    fn no_check_reads_nothing_and_a_stop_is_not_complete() {
        let d = dirty(2 * MIB);
        let r = overwrite(&d, &[Fill::Zero], false, 1, |_| true);
        assert!(r.complete && !r.checked);
        assert_eq!(r.bytes_checked, 0);
        let r = overwrite(&d, &[Fill::Zero], true, 1, |p| p.done_bytes < MIB as u64);
        assert!(!r.complete);
    }
}
