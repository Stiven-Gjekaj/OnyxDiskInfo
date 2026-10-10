//! The capacity test: does the drive hold as much as it claims?
//!
//! A fake flash drive claims more space than its chips have. Its controller
//! maps each address past the real end back onto an address inside it, or it
//! drops the write. A file system on it works until it is full past the real
//! end, and then the files that the person wrote first are gone.
//!
//! The test writes a marked block at each of a set of probe offsets, then
//! reads each one back. A block that reads back with the mark of another
//! offset sits on the same chip as that offset: the drive wraps. A block that
//! reads back as anything else: the drive dropped the write.
//!
//! **The order of the writes matters.** The test writes from the highest
//! offset to the lowest. On a drive that wraps, a high block lands on a low
//! one, and the low block, written later, overwrites it. The low block then
//! reads back right, and the high one reads back with the mark of the low
//! one. Written the other way, a good low block would read back wrong, and
//! the test would blame the real part of the drive.
//!
//! The test saves each block before it writes and writes it back at the end,
//! so a genuine drive keeps its data. On a fake drive, the data past the real
//! end was never there. The test still writes to the drive, and a power cut
//! during it loses what the saved blocks held.

use std::io;

use crate::device::{AlignedBuffer, Device};

/// The number of probe offsets, spread from the start to the end.
pub const PROBES: u64 = 1024;

/// The size of each probe block: 64 KiB.
pub const PROBE_BYTES: u64 = 64 * 1024;

/// The mark at the start of each probe block.
const MAGIC: &[u8; 8] = b"ONYXPROB";

/// What one probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The block read back with the mark of the block at this offset.
    Wraps { onto: u64 },
    /// The block read back as data that the test did not write.
    Dropped,
    /// The read failed.
    ReadError(String),
    /// The write failed.
    WriteError(String),
}

/// The result of a capacity test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapacityReport {
    /// The size that the drive claims, in bytes.
    pub claimed: u64,
    pub probes: u64,
    /// The first probe that failed, and how, in the order of the offsets.
    pub first_fault: Option<(u64, Fault)>,
    /// The number of probes that failed.
    pub faults: u64,
    /// The end of the last probe below the first fault. The real capacity is
    /// at least this, and less than the offset of the first fault.
    pub proven_bytes: u64,
    /// Each saved block went back to the drive.
    pub restored: bool,
}

impl CapacityReport {
    /// The drive holds what it claims, as far as the probes reach.
    pub fn genuine(&self) -> bool {
        self.faults == 0
    }
}

/// The offsets of the probes, in bytes, from low to high.
///
/// The first probe is at offset 0 and the last one ends at the end of the
/// drive. Each one is a multiple of the sector size.
pub fn probe_offsets(size: u64, sector: u32, probes: u64, block: u64) -> Vec<u64> {
    let sector = u64::from(sector.max(1));
    if size < block {
        return Vec::new();
    }
    let last = (size - block) / sector * sector;
    let mut out: Vec<u64> = (0..probes.max(2))
        .map(|i| (u128::from(last) * u128::from(i) / u128::from(probes.max(2) - 1)) as u64)
        .map(|o| o / sector * sector)
        .collect();
    out.dedup();
    out
}

/// A small generator of bytes that no drive makes up by chance.
pub(crate) fn fill(block: &mut [u8], run: u64, offset: u64) {
    let mut state = run ^ offset.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    for chunk in block.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    block[0..8].copy_from_slice(MAGIC);
    block[8..16].copy_from_slice(&run.to_le_bytes());
    block[16..24].copy_from_slice(&offset.to_le_bytes());
}

/// What a block that came back holds.
fn judge(back: &[u8], expected: &[u8], run: u64) -> Option<Fault> {
    if back == expected {
        return None;
    }
    let marked = back.get(0..8) == Some(MAGIC.as_slice())
        && back.get(8..16) == Some(run.to_le_bytes().as_slice());
    if marked {
        let mut onto = [0u8; 8];
        onto.copy_from_slice(&back[16..24]);
        let onto = u64::from_le_bytes(onto);
        let mut written = vec![0u8; back.len()];
        fill(&mut written, run, onto);
        if back == written.as_slice() {
            return Some(Fault::Wraps { onto });
        }
    }
    Some(Fault::Dropped)
}

/// Run the capacity test.
///
/// `run` marks the blocks of this run, so a block that an older run left on
/// the drive does not count as a block of this one. The caller gives a value
/// that changes, such as the time.
pub fn test(device: &dyn Device, run: u64) -> io::Result<CapacityReport> {
    let claimed = device.size();
    let offsets = probe_offsets(claimed, device.sector_size(), PROBES, PROBE_BYTES);
    let block = PROBE_BYTES as usize;

    // Save each block first. A failed read here stops the test before it
    // writes anything.
    let mut saved = Vec::with_capacity(offsets.len());
    let mut buffer = AlignedBuffer::new(block);
    for offset in &offsets {
        device.read_at(buffer.bytes_mut(), *offset)?;
        saved.push(buffer.bytes().to_vec());
    }

    let mut faults: Vec<Option<Fault>> = vec![None; offsets.len()];
    for (index, offset) in offsets.iter().enumerate().rev() {
        fill(buffer.bytes_mut(), run, *offset);
        if let Err(e) = device.write_at(buffer.bytes(), *offset) {
            faults[index] = Some(Fault::WriteError(e.to_string()));
        }
    }
    device.flush()?;

    let mut expected = vec![0u8; block];
    for (index, offset) in offsets.iter().enumerate() {
        if faults[index].is_some() {
            continue;
        }
        fill(&mut expected, run, *offset);
        faults[index] = match device.read_at(buffer.bytes_mut(), *offset) {
            Ok(()) => judge(buffer.bytes(), &expected, run),
            Err(e) => Some(Fault::ReadError(e.to_string())),
        };
    }

    // Put the saved blocks back from high to low, so that on a drive that
    // wraps, the low blocks, which are real, end with their own data.
    let mut restored = true;
    for (offset, data) in offsets.iter().zip(&saved).rev() {
        buffer.bytes_mut().copy_from_slice(data);
        if device.write_at(buffer.bytes(), *offset).is_err() {
            restored = false;
        }
    }
    if device.flush().is_err() {
        restored = false;
    }

    let first = faults.iter().position(Option::is_some);
    let proven_bytes = match first {
        Some(0) => 0,
        Some(i) => offsets[i - 1] + PROBE_BYTES,
        None => claimed,
    };
    Ok(CapacityReport {
        claimed,
        probes: offsets.len() as u64,
        first_fault: first.and_then(|i| faults[i].clone().map(|f| (offsets[i], f))),
        faults: faults.iter().filter(|f| f.is_some()).count() as u64,
        proven_bytes,
        restored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;

    /// A fake drive that claims `claimed` bytes, and maps each address onto
    /// the real ones by the remainder, as most fake flash drives do.
    pub(crate) struct Wrapping {
        pub(crate) real: MemoryDevice,
        pub(crate) claimed: u64,
    }

    impl Device for Wrapping {
        fn size(&self) -> u64 {
            self.claimed
        }
        fn sector_size(&self) -> u32 {
            self.real.sector_size()
        }
        fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
            self.real.read_at(buffer, offset % self.real.size())
        }
        fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
            self.real.write_at(buffer, offset % self.real.size())
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A fake drive that drops each write past the real end, and reads zeros
    /// there.
    struct Dropping {
        real: MemoryDevice,
        claimed: u64,
    }

    impl Device for Dropping {
        fn size(&self) -> u64 {
            self.claimed
        }
        fn sector_size(&self) -> u32 {
            512
        }
        fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
            // The part inside the real end reads back. The rest reads zeros.
            buffer.fill(0);
            let real = self
                .real
                .size()
                .saturating_sub(offset)
                .min(buffer.len() as u64);
            if real > 0 {
                self.real.read_at(&mut buffer[..real as usize], offset)?;
            }
            Ok(())
        }
        fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
            // The part inside the real end is kept. The rest is dropped.
            let real = self
                .real
                .size()
                .saturating_sub(offset)
                .min(buffer.len() as u64);
            if real > 0 {
                self.real.write_at(&buffer[..real as usize], offset)?;
            }
            Ok(())
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn the_probes_start_at_zero_and_end_at_the_end() {
        let offsets = probe_offsets(64 * MIB, 512, PROBES, PROBE_BYTES);
        assert_eq!(offsets.len(), 1024);
        assert_eq!(offsets[0], 0);
        assert_eq!(*offsets.last().unwrap() + PROBE_BYTES, 64 * MIB);
        assert!(offsets.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_genuine_drive_passes_and_keeps_its_data() {
        let d = MemoryDevice::new((64 * MIB) as usize, 512);
        let pattern: Vec<u8> = (0..64 * MIB).map(|i| (i % 251) as u8).collect();
        d.write_at(&pattern, 0).unwrap();
        let r = test(&d, 42).unwrap();
        assert!(r.genuine(), "{r:?}");
        assert_eq!(r.proven_bytes, 64 * MIB);
        assert!(r.restored);
        assert_eq!(d.contents(), pattern, "the test left the drive changed");
    }

    #[test]
    fn a_drive_that_wraps_is_found_and_the_real_end_is_bracketed() {
        let real = 16 * MIB;
        let d = Wrapping {
            real: MemoryDevice::new(real as usize, 512),
            claimed: 64 * MIB,
        };
        let r = test(&d, 42).unwrap();
        assert!(!r.genuine());
        let (offset, fault) = r.first_fault.clone().unwrap();
        assert!(
            matches!(fault, Fault::Wraps { onto } if onto == offset % real),
            "{fault:?}"
        );
        // The real end lies between what the test proved and the first fault.
        assert!(
            r.proven_bytes <= real && real <= offset + PROBE_BYTES,
            "{r:?}"
        );
        assert!(r.proven_bytes > real - 2 * 64 * MIB / 1023);
        // Three quarters of the probes sit past the real end.
        assert!(r.faults >= 760 && r.faults <= 770, "{}", r.faults);
    }

    #[test]
    fn the_real_part_of_a_drive_that_wraps_keeps_its_data() {
        let real = 16 * MIB;
        let d = Wrapping {
            real: MemoryDevice::new(real as usize, 512),
            claimed: 64 * MIB,
        };
        let pattern: Vec<u8> = (0..real).map(|i| (i % 249) as u8).collect();
        d.real.write_at(&pattern, 0).unwrap();
        test(&d, 7).unwrap();
        assert_eq!(d.real.contents(), pattern);
    }

    #[test]
    fn a_drive_that_drops_writes_is_found() {
        let d = Dropping {
            real: MemoryDevice::new((8 * MIB) as usize, 512),
            claimed: 32 * MIB,
        };
        let r = test(&d, 42).unwrap();
        assert!(!r.genuine());
        assert!(matches!(r.first_fault, Some((_, Fault::Dropped))));
        assert!(r.proven_bytes <= 8 * MIB);
    }

    #[test]
    fn a_block_from_an_older_run_does_not_count() {
        let mut old = vec![0u8; PROBE_BYTES as usize];
        fill(&mut old, 1, 4096);
        let mut expected = vec![0u8; PROBE_BYTES as usize];
        fill(&mut expected, 2, 0);
        assert_eq!(judge(&old, &expected, 2), Some(Fault::Dropped));
    }

    #[test]
    fn a_drive_smaller_than_one_probe_gets_no_probe() {
        assert!(probe_offsets(1000, 512, PROBES, PROBE_BYTES).is_empty());
    }
}
