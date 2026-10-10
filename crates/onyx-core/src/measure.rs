//! A record of a drive that reports no health: what Onyx measured itself.
//!
//! Onyx reads a sample of places spread over the whole drive, and times each
//! read. It writes nothing. The full surface scan of P4 reads every block;
//! this sample takes seconds and finds a drive that is already failing.
//!
//! A good result says only that the reads in the sample worked. It says
//! nothing about the blocks between them, and nothing about wear. That is
//! why every record from here has a low confidence.

use crate::health::{Assessment, Verdict};

/// The number of places that a sample reads.
pub const SAMPLES: u64 = 64;

/// The bytes that each read takes: 1 MiB.
pub const CHUNK_BYTES: u64 = 1024 * 1024;

/// A read slower than this is a caution, in microseconds.
///
/// One second for one MiB is less than 1 MB/s. The slowest USB 2 flash
/// drive reads more than ten times faster, so a read this slow is a block
/// that the drive had to retry.
pub const SLOW_READ_MICROS: u64 = 1_000_000;

/// The offsets that a sample reads, in bytes, in order.
///
/// The first read is at the start of the drive and the last one ends at the
/// end, so a drive that claims more space than it has fails the last read.
/// Each offset is a multiple of the sector size. A drive smaller than one
/// chunk gives one read at offset zero.
pub fn plan(size_bytes: u64, sector_size: u32, samples: u64, chunk: u64) -> Vec<u64> {
    let sector = u64::from(sector_size.max(1));
    if size_bytes < chunk || samples < 2 {
        return vec![0];
    }
    let last = (size_bytes - chunk) / sector * sector;
    let mut out: Vec<u64> = (0..samples)
        .map(|i| (u128::from(last) * u128::from(i) / u128::from(samples - 1)) as u64)
        .map(|offset| offset / sector * sector)
        .collect();
    out.dedup();
    out
}

/// What one read of the sample gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    /// The read worked and took this many microseconds.
    Done { offset: u64, micros: u64 },
    /// The read failed, and the host gave this message.
    Failed { offset: u64, message: String },
}

/// The result of one sample.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Measurement {
    pub reads: Vec<Read>,
    /// The bytes that each read asked for.
    pub chunk_bytes: u64,
}

impl Measurement {
    /// The reads that failed.
    pub fn errors(&self) -> impl Iterator<Item = (u64, &str)> {
        self.reads.iter().filter_map(|r| match r {
            Read::Failed { offset, message } => Some((*offset, message.as_str())),
            Read::Done { .. } => None,
        })
    }

    /// The time of each read that worked, in microseconds, in order.
    fn times(&self) -> Vec<u64> {
        let mut times: Vec<u64> = self
            .reads
            .iter()
            .filter_map(|r| match r {
                Read::Done { micros, .. } => Some(*micros),
                Read::Failed { .. } => None,
            })
            .collect();
        times.sort_unstable();
        times
    }

    /// The median time of a read, in microseconds.
    pub fn median_micros(&self) -> Option<u64> {
        let times = self.times();
        times.get(times.len() / 2).copied()
    }

    /// The slowest read that worked, in microseconds.
    pub fn max_micros(&self) -> Option<u64> {
        self.times().last().copied()
    }

    /// The median speed, in bytes per second.
    pub fn median_bytes_per_second(&self) -> Option<u64> {
        let micros = self.median_micros()?.max(1);
        Some((u128::from(self.chunk_bytes) * 1_000_000 / u128::from(micros)) as u64)
    }

    /// The verdict of the sample.
    ///
    /// - Bad: a read failed. A drive that cannot read a block that it
    ///   claims to have is losing data, or claims space it does not have.
    /// - Caution: a read took a second or more.
    pub fn assessment(&self) -> Assessment {
        let mut out = Assessment::new();
        let failed = self.errors().count();
        if failed > 0 {
            let (offset, message) = self.errors().next().unwrap_or((0, ""));
            out.add(
                Verdict::Bad,
                format!(
                    "{failed} of {} reads failed. The first one at byte {offset}: {message}",
                    self.reads.len()
                ),
            );
        }
        let slow = self
            .times()
            .iter()
            .filter(|t| **t >= SLOW_READ_MICROS)
            .count();
        if slow > 0 {
            out.add(
                Verdict::Caution,
                format!(
                    "{slow} reads took a second or more. The slowest took {} ms.",
                    self.max_micros().unwrap_or(0) / 1000
                ),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_starts_at_zero_and_its_last_read_ends_at_the_end() {
        let size = 32_010_928_128;
        let offsets = plan(size, 512, SAMPLES, CHUNK_BYTES);
        assert_eq!(offsets.len(), 64);
        assert_eq!(offsets[0], 0);
        let last = *offsets.last().unwrap();
        assert!(last + CHUNK_BYTES <= size);
        assert!(
            size - (last + CHUNK_BYTES) < 512,
            "the last read stops short"
        );
    }

    #[test]
    fn each_offset_is_a_whole_sector_and_they_rise() {
        for sector in [512u32, 4096] {
            let offsets = plan(1_000_204_886_016, sector, SAMPLES, CHUNK_BYTES);
            assert!(offsets.iter().all(|o| o % u64::from(sector) == 0));
            assert!(offsets.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn a_drive_smaller_than_one_chunk_gets_one_read() {
        assert_eq!(plan(4096, 512, SAMPLES, CHUNK_BYTES), [0]);
    }

    #[test]
    fn a_small_drive_gets_no_read_twice() {
        let offsets = plan(CHUNK_BYTES + 3 * 512, 512, SAMPLES, CHUNK_BYTES);
        let mut unique = offsets.clone();
        unique.dedup();
        assert_eq!(offsets, unique);
        assert_eq!(offsets.len(), 4);
    }

    fn done(offset: u64, millis: u64) -> Read {
        Read::Done {
            offset,
            micros: millis * 1000,
        }
    }

    #[test]
    fn a_sample_with_no_error_and_no_slow_read_is_good() {
        let m = Measurement {
            reads: vec![done(0, 30), done(1, 40), done(2, 35)],
            chunk_bytes: CHUNK_BYTES,
        };
        assert_eq!(m.assessment().verdict, Verdict::Good);
        assert_eq!(m.median_micros(), Some(35_000));
        assert_eq!(m.max_micros(), Some(40_000));
        assert_eq!(m.median_bytes_per_second(), Some(29_959_314));
    }

    #[test]
    fn a_failed_read_makes_the_drive_bad_and_names_where() {
        let m = Measurement {
            reads: vec![
                done(0, 30),
                Read::Failed {
                    offset: 31_000_000_000,
                    message: "Input/output error".to_string(),
                },
            ],
            chunk_bytes: CHUNK_BYTES,
        };
        let a = m.assessment();
        assert_eq!(a.verdict, Verdict::Bad);
        assert!(
            a.reasons[0].contains("1 of 2 reads failed"),
            "{:?}",
            a.reasons
        );
        assert!(a.reasons[0].contains("31000000000"));
    }

    #[test]
    fn a_read_of_a_second_or_more_is_a_caution() {
        let m = Measurement {
            reads: vec![done(0, 30), done(1, 999)],
            chunk_bytes: CHUNK_BYTES,
        };
        assert_eq!(m.assessment().verdict, Verdict::Good);
        let m = Measurement {
            reads: vec![done(0, 30), done(1, 1000)],
            chunk_bytes: CHUNK_BYTES,
        };
        let a = m.assessment();
        assert_eq!(a.verdict, Verdict::Caution);
        assert!(a.reasons[0].contains("1000 ms"), "{:?}", a.reasons);
    }

    #[test]
    fn a_sample_where_every_read_failed_has_no_times() {
        let m = Measurement {
            reads: vec![Read::Failed {
                offset: 0,
                message: "gone".to_string(),
            }],
            chunk_bytes: CHUNK_BYTES,
        };
        assert_eq!(m.median_micros(), None);
        assert_eq!(m.median_bytes_per_second(), None);
        assert_eq!(m.assessment().verdict, Verdict::Bad);
    }
}
