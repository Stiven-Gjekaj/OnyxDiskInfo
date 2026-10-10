//! A drive that is open, as the target that the writers of Burnout take.
//!
//! A raw device reports no length of its own, so the caller gives it. A drive
//! that the host opens without its cache takes a buffer only at an address
//! that its sector size divides, so each read and each write goes through an
//! aligned buffer of its own.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

use burnout_core::BlockTarget;

/// The alignment of the buffer: every sector size that a drive uses divides
/// it.
const ALIGN: usize = 4096;

/// A buffer whose start is a multiple of [`ALIGN`].
#[derive(Debug, Default)]
struct Aligned {
    storage: Vec<u8>,
    start: usize,
}

impl Aligned {
    fn slice(&mut self, length: usize) -> &mut [u8] {
        if self.storage.len() < self.start + length || self.storage.is_empty() {
            self.storage = vec![0; length + ALIGN];
            self.start = self.storage.as_ptr().align_offset(ALIGN);
        }
        &mut self.storage[self.start..self.start + length]
    }
}

/// A drive or an image, open for a write, as a [`BlockTarget`].
#[derive(Debug)]
pub struct DriveTarget {
    file: File,
    length: u64,
    sector: u32,
    position: u64,
    buffer: Aligned,
}

impl DriveTarget {
    /// Take a file that is open for reading and writing, with the size and
    /// the sector size that the drive list reported.
    pub fn new(file: File, length: u64, sector: u32) -> Self {
        DriveTarget {
            file,
            length,
            sector,
            position: 0,
            buffer: Aligned::default(),
        }
    }
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buffer, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buffer, offset)
}

#[cfg(windows)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buffer, offset)
}

impl Read for DriveTarget {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let length = out
            .len()
            .min(self.length.saturating_sub(self.position) as usize);
        if length == 0 {
            return Ok(0);
        }
        let aligned = self.buffer.slice(length);
        let n = read_at(&self.file, aligned, self.position)?;
        out[..n].copy_from_slice(&aligned[..n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl Write for DriveTarget {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.position + data.len() as u64 > self.length {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "a write past the end of the drive",
            ));
        }
        let aligned = self.buffer.slice(data.len());
        aligned.copy_from_slice(data);
        let n = write_at(&self.file, aligned, self.position)?;
        self.position += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for DriveTarget {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.length.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        self.position = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.position)
    }
}

impl BlockTarget for DriveTarget {
    fn logical_sector_size(&self) -> u32 {
        self.sector
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn sync(&mut self) -> burnout_core::Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str, size: u64) -> (DriveTarget, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("onyx-target-{}-{name}", std::process::id()));
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.set_len(size).unwrap();
        (DriveTarget::new(file, size, 512), path)
    }

    #[test]
    fn a_write_reads_back_at_its_place() {
        let (mut t, path) = target("place", 8192);
        t.seek(SeekFrom::Start(1024)).unwrap();
        t.write_all(&[7u8; 512]).unwrap();
        t.seek(SeekFrom::Start(1024)).unwrap();
        let mut back = [0u8; 512];
        t.read_exact(&mut back).unwrap();
        assert_eq!(back, [7u8; 512]);
        t.sync().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_length_is_the_one_given_and_not_the_one_of_the_file() {
        let (t, path) = target("length", 8192);
        let t = DriveTarget::new(t.file, 4096, 512);
        assert_eq!(t.length(), 4096);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_write_past_the_end_is_refused_and_a_read_there_gives_nothing() {
        let (mut t, path) = target("end", 4096);
        t.seek(SeekFrom::End(-512)).unwrap();
        assert!(t.write_all(&[1u8; 1024]).is_err());
        t.seek(SeekFrom::Start(4096)).unwrap();
        let mut back = [0u8; 512];
        assert_eq!(t.read(&mut back).unwrap(), 0);
        assert!(t.seek(SeekFrom::Current(-5000)).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
