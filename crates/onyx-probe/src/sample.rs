//! The shim that reads a sample of a drive.
//!
//! This file reads and decides nothing. The plan of the reads and the verdict
//! come from `onyx_core::measure`, whose tests run on all three hosts.
//!
//! It opens the drive for reading only, through [`crate::open`]. No call
//! here can write to it.

use std::fs::File;
use std::io;
use std::time::Instant;

use onyx_core::measure::{self, Measurement, Read};
use onyx_core::{DriveInfo, Result};

use crate::open::{Access, open_drive, refused};

/// The alignment of the buffer. A drive that the host opens without its
/// cache needs the buffer, the offset, and the length to be multiples of the
/// logical sector size, and 4096 covers every sector size that a drive uses.
const ALIGN: usize = 4096;

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buffer.is_empty() {
        match file.seek_read(buffer, offset)? {
            0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            n => {
                buffer = &mut buffer[n..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn read_at(_file: &File, _buffer: &mut [u8], _offset: u64) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// The message of a failed read, as a person reads it.
fn message(e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::UnexpectedEof {
        "the drive gave fewer bytes than it claims to have".to_string()
    } else {
        e.to_string()
    }
}

/// Read the sample of one drive, and time each read.
pub fn sample(drive: &DriveInfo) -> Result<Measurement> {
    let file = open_drive(drive, Access::Read)?.file;
    let chunk = measure::CHUNK_BYTES;
    let offsets = measure::plan(
        drive.size_bytes,
        drive.logical_sector_size,
        measure::SAMPLES,
        chunk,
    );

    let mut storage = vec![0u8; chunk as usize + ALIGN];
    let start = storage.as_ptr().align_offset(ALIGN);
    let length = (chunk as usize).min(drive.size_bytes as usize);
    let buffer = &mut storage[start..start + length];

    let mut reads = Vec::with_capacity(offsets.len());
    for offset in offsets {
        let began = Instant::now();
        let result = read_at(&file, buffer, offset);
        let micros = began.elapsed().as_micros() as u64;
        reads.push(match result {
            Ok(()) => Read::Done { offset, micros },
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return Err(refused(e, &drive.node));
            }
            Err(e) => Read::Failed {
                offset,
                message: message(&e),
            },
        });
    }
    Ok(Measurement {
        reads,
        chunk_bytes: length as u64,
    })
}
