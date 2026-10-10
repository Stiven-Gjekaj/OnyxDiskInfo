//! The benchmarks: how fast a drive reads and writes.
//!
//! The four jobs are the default jobs of CrystalDiskMark, so a person can put
//! the two tools side by side. Each job runs for a fixed time over a fixed
//! span of the drive, and counts what it did.
//!
//! The queue depth is the number of threads. Each thread sends one blocking
//! command at a time, so `queue` threads keep `queue` commands in flight, as
//! long as the host opened the drive without its cache.

use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::capacity::fill;
use crate::device::{AlignedBuffer, Device};

/// The order of the offsets of a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Sequential,
    Random,
}

/// One benchmark job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    /// The name that CrystalDiskMark gives the job.
    pub name: &'static str,
    pub pattern: Pattern,
    /// The size of each command, in bytes.
    pub block: u64,
    /// The number of commands in flight.
    pub queue: usize,
    pub write: bool,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// The four read jobs of CrystalDiskMark. [`Job::writing`] makes the write
/// job of each one.
pub const JOBS: [Job; 4] = [
    Job {
        name: "SEQ1M Q8T1",
        pattern: Pattern::Sequential,
        block: MIB,
        queue: 8,
        write: false,
    },
    Job {
        name: "SEQ1M Q1T1",
        pattern: Pattern::Sequential,
        block: MIB,
        queue: 1,
        write: false,
    },
    Job {
        name: "RND4K Q32T1",
        pattern: Pattern::Random,
        block: 4 * KIB,
        queue: 32,
        write: false,
    },
    Job {
        name: "RND4K Q1T1",
        pattern: Pattern::Random,
        block: 4 * KIB,
        queue: 1,
        write: false,
    },
];

impl Job {
    /// The same job, as a write.
    pub fn writing(self) -> Job {
        Job {
            write: true,
            ..self
        }
    }
}

/// The span of a drive that a job uses: 1 GiB, as CrystalDiskMark uses by
/// default, or the whole drive when it is smaller.
pub const SPAN_BYTES: u64 = 1024 * MIB;

/// What one job did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobResult {
    pub job: Job,
    pub bytes: u64,
    pub commands: u64,
    pub micros: u64,
    /// The sum of the time of each command, in microseconds.
    pub busy_micros: u64,
}

impl JobResult {
    /// Bytes per second.
    pub fn bytes_per_second(&self) -> u64 {
        (u128::from(self.bytes) * 1_000_000 / u128::from(self.micros.max(1))) as u64
    }

    /// Commands per second.
    pub fn iops(&self) -> u64 {
        (u128::from(self.commands) * 1_000_000 / u128::from(self.micros.max(1))) as u64
    }

    /// The mean time of one command, in microseconds.
    pub fn mean_latency_micros(&self) -> u64 {
        self.busy_micros / self.commands.max(1)
    }
}

/// A small generator of offsets, one for each thread.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Run one job for `duration`, over the span from `start` of `span` bytes.
///
/// The span shrinks to whole blocks. A span smaller than one block is an
/// error. A write job writes over the span: the caller makes sure that the
/// span is a test file or a drive that the person gave up.
pub fn run(
    device: &dyn Device,
    job: Job,
    start: u64,
    span: u64,
    duration: Duration,
    seed: u64,
) -> io::Result<JobResult> {
    let blocks = span.min(device.size().saturating_sub(start)) / job.block;
    if blocks == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the span holds no block of {} bytes", job.block),
        ));
    }
    let next_block = AtomicU64::new(0);
    let totals = Mutex::new((0u64, 0u64, 0u64));
    let failure: Mutex<Option<io::Error>> = Mutex::new(None);
    let began = Instant::now();

    std::thread::scope(|scope| {
        for worker in 0..job.queue.max(1) {
            let next_block = &next_block;
            let totals = &totals;
            let failure = &failure;
            scope.spawn(move || {
                let mut buffer = AlignedBuffer::new(job.block as usize);
                if job.write {
                    fill(buffer.bytes_mut(), seed, worker as u64);
                }
                let mut random = Random(seed ^ (worker as u64 + 1).wrapping_mul(0x9e37_79b9));
                let (mut bytes, mut commands, mut busy) = (0u64, 0u64, 0u64);
                while began.elapsed() < duration {
                    let index = match job.pattern {
                        Pattern::Sequential => next_block.fetch_add(1, Ordering::Relaxed) % blocks,
                        Pattern::Random => random.next() % blocks,
                    };
                    let offset = start + index * job.block;
                    let t = Instant::now();
                    let result = if job.write {
                        device.write_at(buffer.bytes(), offset)
                    } else {
                        device.read_at(buffer.bytes_mut(), offset)
                    };
                    busy += t.elapsed().as_micros() as u64;
                    if let Err(e) = result {
                        if let Ok(mut slot) = failure.lock() {
                            slot.get_or_insert(e);
                        }
                        break;
                    }
                    bytes += job.block;
                    commands += 1;
                }
                if let Ok(mut t) = totals.lock() {
                    t.0 += bytes;
                    t.1 += commands;
                    t.2 += busy;
                }
            });
        }
    });
    if job.write {
        device.flush()?;
    }
    let micros = began.elapsed().as_micros() as u64;

    if let Some(e) = failure.into_inner().ok().flatten() {
        return Err(e);
    }
    let (bytes, commands, busy_micros) = totals.into_inner().unwrap_or((0, 0, 0));
    Ok(JobResult {
        job,
        bytes,
        commands,
        micros,
        busy_micros,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MemoryDevice;

    /// A device that records each command that it gets.
    struct Recording {
        inner: MemoryDevice,
        log: Mutex<Vec<(u64, usize, bool)>>,
    }

    impl Recording {
        fn new(size: usize) -> Self {
            Recording {
                inner: MemoryDevice::new(size, 512),
                log: Mutex::new(Vec::new()),
            }
        }
        fn log(&self) -> Vec<(u64, usize, bool)> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Device for Recording {
        fn size(&self) -> u64 {
            self.inner.size()
        }
        fn sector_size(&self) -> u32 {
            512
        }
        fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
            self.log.lock().unwrap().push((offset, buffer.len(), false));
            self.inner.read_at(buffer, offset)
        }
        fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
            self.log.lock().unwrap().push((offset, buffer.len(), true));
            self.inner.write_at(buffer, offset)
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    const SHORT: Duration = Duration::from_millis(30);

    #[test]
    fn the_jobs_are_the_four_of_crystaldiskmark() {
        let names: Vec<&str> = JOBS.iter().map(|j| j.name).collect();
        assert_eq!(
            names,
            ["SEQ1M Q8T1", "SEQ1M Q1T1", "RND4K Q32T1", "RND4K Q1T1"]
        );
        assert!(JOBS.iter().all(|j| !j.write));
        assert!(JOBS[0].writing().write);
    }

    #[test]
    fn a_sequential_job_at_queue_one_reads_each_block_in_order() {
        let d = Recording::new(8 * MIB as usize);
        let r = run(&d, JOBS[1], 0, 4 * MIB, SHORT, 1).unwrap();
        let log = d.log();
        assert!(r.commands > 4, "the job wrapped round the span");
        for (i, (offset, length, write)) in log.iter().enumerate() {
            assert_eq!(*offset, (i as u64 % 4) * MIB);
            assert_eq!(*length, MIB as usize);
            assert!(!write);
        }
        assert_eq!(r.bytes, r.commands * MIB);
    }

    #[test]
    fn a_random_job_stays_in_its_span_and_on_whole_blocks() {
        let d = Recording::new(8 * MIB as usize);
        run(&d, JOBS[2], MIB, 2 * MIB, SHORT, 7).unwrap();
        let log = d.log();
        assert!(log.len() > 32);
        for (offset, length, _) in &log {
            assert!(*offset >= MIB && offset + *length as u64 <= 3 * MIB);
            assert_eq!(offset % (4 * KIB), 0);
        }
        let distinct: std::collections::BTreeSet<u64> = log.iter().map(|l| l.0).collect();
        assert!(distinct.len() > 100, "the offsets do not spread");
    }

    #[test]
    fn a_read_job_never_writes_and_a_write_job_never_reads() {
        let d = Recording::new(4 * MIB as usize);
        run(&d, JOBS[3], 0, 4 * MIB, SHORT, 1).unwrap();
        assert!(d.log().iter().all(|l| !l.2));
        let d = Recording::new(4 * MIB as usize);
        run(&d, JOBS[3].writing(), 0, 4 * MIB, SHORT, 1).unwrap();
        assert!(d.log().iter().all(|l| l.2));
    }

    #[test]
    fn the_rates_come_from_the_counts_and_the_time() {
        let r = JobResult {
            job: JOBS[0],
            bytes: 500 * MIB,
            commands: 500,
            micros: 1_000_000,
            busy_micros: 4_000_000,
        };
        assert_eq!(r.bytes_per_second(), 500 * MIB);
        assert_eq!(r.iops(), 500);
        assert_eq!(r.mean_latency_micros(), 8_000);
    }

    #[test]
    fn a_span_smaller_than_a_block_is_an_error() {
        let d = MemoryDevice::new(4096, 512);
        assert!(run(&d, JOBS[0], 0, SPAN_BYTES, SHORT, 1).is_err());
    }

    #[test]
    fn a_failed_command_ends_the_job_with_its_error() {
        struct Broken;
        impl Device for Broken {
            fn size(&self) -> u64 {
                MIB
            }
            fn sector_size(&self) -> u32 {
                512
            }
            fn read_at(&self, _: &mut [u8], _: u64) -> io::Result<()> {
                Err(io::Error::other("gone"))
            }
            fn write_at(&self, _: &[u8], _: u64) -> io::Result<()> {
                Ok(())
            }
            fn flush(&self) -> io::Result<()> {
                Ok(())
            }
        }
        let e = run(&Broken, JOBS[3], 0, MIB, SHORT, 1).unwrap_err();
        assert_eq!(e.to_string(), "gone");
    }
}
