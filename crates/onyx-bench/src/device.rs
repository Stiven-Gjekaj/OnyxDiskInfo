//! The thing that a disk test reads and writes.

use std::fs::File;
use std::io;
use std::sync::Mutex;

/// A block device, or something that acts like one.
///
/// Each call takes `&self` and names its offset, so a benchmark can send
/// reads from many threads at once, the way a queue of commands does.
pub trait Device: Sync {
    /// The size in bytes, as the device claims it.
    fn size(&self) -> u64;

    /// The logical sector size in bytes. Each offset and each length that a
    /// test uses is a multiple of it.
    fn sector_size(&self) -> u32;

    /// Fill `buffer` from `offset`. A short read is an error.
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()>;

    /// Write all of `buffer` at `offset`.
    fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()>;

    /// Wait until each write is on the medium.
    fn flush(&self) -> io::Result<()>;
}

/// A device in memory, for tests.
#[derive(Debug)]
pub struct MemoryDevice {
    bytes: Mutex<Vec<u8>>,
    sector: u32,
}

impl MemoryDevice {
    /// A device of `size` bytes of zero.
    pub fn new(size: usize, sector: u32) -> Self {
        MemoryDevice {
            bytes: Mutex::new(vec![0; size]),
            sector,
        }
    }

    /// A copy of the contents.
    pub fn contents(&self) -> Vec<u8> {
        self.bytes.lock().map(|b| b.clone()).unwrap_or_default()
    }
}

fn past_end(offset: u64, length: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("{length} bytes at {offset} go past the end"),
    )
}

impl Device for MemoryDevice {
    fn size(&self) -> u64 {
        self.bytes.lock().map(|b| b.len() as u64).unwrap_or(0)
    }

    fn sector_size(&self) -> u32 {
        self.sector
    }

    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        let bytes = self
            .bytes
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?;
        let start = usize::try_from(offset).map_err(|_| past_end(offset, buffer.len()))?;
        let source = bytes
            .get(start..start + buffer.len())
            .ok_or_else(|| past_end(offset, buffer.len()))?;
        buffer.copy_from_slice(source);
        Ok(())
    }

    fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
        let mut bytes = self
            .bytes
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?;
        let start = usize::try_from(offset).map_err(|_| past_end(offset, buffer.len()))?;
        let target = bytes
            .get_mut(start..start + buffer.len())
            .ok_or_else(|| past_end(offset, buffer.len()))?;
        target.copy_from_slice(buffer);
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
}

/// A device on an open file: a drive that the host opened, or an image.
#[derive(Debug)]
pub struct FileDevice {
    file: File,
    size: u64,
    sector: u32,
}

impl FileDevice {
    /// Take a file that is open already, with the size and the sector size
    /// that the drive list reported. A device file reports no size of its
    /// own on every host, so the caller gives it.
    pub fn new(file: File, size: u64, sector: u32) -> Self {
        FileDevice { file, size, sector }
    }
}

#[cfg(unix)]
impl Device for FileDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn sector_size(&self) -> u32 {
        self.sector
    }

    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        use std::os::unix::fs::FileExt;
        self.file.read_exact_at(buffer, offset)
    }

    fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<()> {
        use std::os::unix::fs::FileExt;
        self.file.write_all_at(buffer, offset)
    }

    fn flush(&self) -> io::Result<()> {
        self.file.sync_all()
    }
}

#[cfg(windows)]
impl Device for FileDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn sector_size(&self) -> u32 {
        self.sector
    }

    fn read_at(&self, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buffer.is_empty() {
            match self.file.seek_read(buffer, offset)? {
                0 => return Err(past_end(offset, buffer.len())),
                n => {
                    buffer = &mut buffer[n..];
                    offset += n as u64;
                }
            }
        }
        Ok(())
    }

    fn write_at(&self, mut buffer: &[u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buffer.is_empty() {
            match self.file.seek_write(buffer, offset)? {
                0 => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                n => {
                    buffer = &buffer[n..];
                    offset += n as u64;
                }
            }
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        self.file.sync_all()
    }
}

/// A buffer whose start is a multiple of 4096 bytes.
///
/// A drive that the host opens with `O_DIRECT`, or without a cache on
/// Windows, takes a buffer aligned to its sector size. 4096 covers every
/// sector size that a drive uses.
#[derive(Debug)]
pub struct AlignedBuffer {
    storage: Vec<u8>,
    start: usize,
    length: usize,
}

/// The alignment of an [`AlignedBuffer`].
pub const ALIGN: usize = 4096;

impl AlignedBuffer {
    /// A buffer of `length` bytes of zero.
    pub fn new(length: usize) -> Self {
        let storage = vec![0u8; length + ALIGN];
        let start = storage.as_ptr().align_offset(ALIGN);
        AlignedBuffer {
            storage,
            start,
            length,
        }
    }

    /// The bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.storage[self.start..self.start + self.length]
    }

    /// The bytes, to fill.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.storage[self.start..self.start + self.length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_gives_back_what_was_written() {
        let d = MemoryDevice::new(8192, 512);
        d.write_at(&[7u8; 512], 512).unwrap();
        let mut back = [0u8; 512];
        d.read_at(&mut back, 512).unwrap();
        assert_eq!(back, [7u8; 512]);
        assert_eq!(d.size(), 8192);
    }

    #[test]
    fn memory_refuses_a_read_or_a_write_past_the_end() {
        let d = MemoryDevice::new(1024, 512);
        let mut back = [0u8; 512];
        assert!(d.read_at(&mut back, 1024).is_err());
        assert!(d.write_at(&[0u8; 512], 768).is_err());
    }

    #[test]
    fn a_file_device_reads_and_writes_at_an_offset() {
        let path = std::env::temp_dir().join(format!("onyx-device-{}", std::process::id()));
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.set_len(8192).unwrap();
        let d = FileDevice::new(file, 8192, 512);
        d.write_at(&[9u8; 1024], 4096).unwrap();
        d.flush().unwrap();
        let mut back = [0u8; 1024];
        d.read_at(&mut back, 4096).unwrap();
        assert_eq!(back, [9u8; 1024]);
        assert!(
            d.read_at(&mut back, 7680).is_err(),
            "a short read is an error"
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_aligned_buffer_starts_on_a_multiple_of_4096() {
        for length in [512, 4096, 1 << 20] {
            let mut b = AlignedBuffer::new(length);
            assert_eq!(b.bytes().as_ptr() as usize % ALIGN, 0);
            assert_eq!(b.bytes_mut().len(), length);
        }
    }
}
