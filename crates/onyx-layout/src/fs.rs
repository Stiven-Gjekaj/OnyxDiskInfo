//! The file systems that Onyx writes itself: FAT12, FAT16, FAT32, and exFAT.
//!
//! These are the file systems of removable drives, and each host reads all
//! four. FAT32 and exFAT come from Burnout, whose CI mounts what they write
//! and checks it with the tools of three hosts. FAT12 and FAT16 come from
//! `fatfs`, over the same target of whole sectors.

use std::io::{Read, Seek, SeekFrom, Write};

use burnout_core::{BlockTarget, MemorySource, SectorIo};
use burnout_layout::{ExfatOptions, Fat32Options, format_fat32, write_exfat};
use fatfs::{FatType, FormatVolumeOptions};
use onyx_core::{Error, Result};

/// A file system that Onyx writes itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSystem {
    Fat12,
    Fat16,
    Fat32,
    Exfat,
}

impl FileSystem {
    /// The name that a person types and reads.
    pub fn name(self) -> &'static str {
        match self {
            FileSystem::Fat12 => "fat12",
            FileSystem::Fat16 => "fat16",
            FileSystem::Fat32 => "fat32",
            FileSystem::Exfat => "exfat",
        }
    }

    /// The kind that the partition table gives the partition.
    pub fn kind(self) -> crate::table::Kind {
        match self {
            FileSystem::Fat12 => crate::table::Kind::Fat12,
            FileSystem::Fat16 => crate::table::Kind::Fat16,
            FileSystem::Fat32 => crate::table::Kind::Fat32,
            FileSystem::Exfat => crate::table::Kind::Exfat,
        }
    }
}

/// What a new volume is called and where it sits.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    /// The name that a host shows. Each file system keeps 11 characters.
    pub label: &'a str,
    /// The volume serial number. Hosts tell volumes apart by it.
    pub serial: u32,
    /// The sector of the drive where the partition starts.
    pub first_sector: u64,
}

fn burnout(error: burnout_core::Error) -> Error {
    Error::host("format", error.to_string())
}

/// The label as FAT keeps it: 11 bytes of upper case ASCII, padded with
/// spaces. A character that a label cannot hold becomes `_`.
fn fat_label(label: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    for (slot, c) in out.iter_mut().zip(label.chars()) {
        *slot = match c.to_ascii_uppercase() {
            c @ ('A'..='Z' | '0'..='9' | ' ' | '_' | '-') => c as u8,
            _ => b'_',
        };
    }
    out
}

/// Where the boot sector of each FAT keeps the sectors before the volume.
const HIDDEN_SECTORS_AT: u64 = 28;

/// A disk that keeps only its first sector and discards each other write.
///
/// `fatfs` takes the FAT type as a hint for the size of a cluster, and then
/// writes the type that the cluster count gives. A dry run on this disk finds
/// that type before a byte of the drive changes.
struct DryRun {
    first: [u8; 512],
    position: u64,
    length: u64,
}

impl Read for DryRun {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        out.fill(0);
        self.position += out.len() as u64;
        Ok(out.len())
    }
}

impl Write for DryRun {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for (i, byte) in data.iter().enumerate() {
            if let Some(slot) = self.first.get_mut(self.position as usize + i) {
                *slot = *byte;
            }
        }
        self.position += data.len() as u64;
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for DryRun {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.position = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.length.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        }
        .ok_or_else(|| std::io::Error::other("a seek before the start"))?;
        Ok(self.position)
    }
}

/// The name that the boot sector of FAT12 and FAT16 gives its type.
fn type_name(fat: FatType) -> &'static [u8; 8] {
    match fat {
        FatType::Fat12 => b"FAT12   ",
        FatType::Fat16 => b"FAT16   ",
        FatType::Fat32 => b"FAT32   ",
    }
}

/// Format FAT12 or FAT16 with `fatfs`, then give the boot sector the place
/// of the partition, which `fatfs` leaves at zero. Windows refuses to mount a
/// FAT volume whose boot sector does not say where it starts.
fn format_small_fat<T: BlockTarget>(volume: T, fat: FatType, options: &Options<'_>) -> Result<()> {
    let sector = volume.logical_sector_size();
    let sectors = u32::try_from(volume.length() / u64::from(sector))
        .map_err(|_| Error::host("format", "FAT12 and FAT16 count sectors in 32 bits"))?;
    let format = || {
        FormatVolumeOptions::new()
            .bytes_per_sector(sector as u16)
            .total_sectors(sectors)
            .fat_type(fat)
            .volume_id(options.serial)
            .volume_label(fat_label(options.label))
    };
    let does_not_fit = |why: String| {
        Error::host(
            "format",
            format!("the partition does not fit {fat:?}: {why}"),
        )
    };
    let mut dry = DryRun {
        first: [0; 512],
        position: 0,
        length: volume.length(),
    };
    fatfs::format_volume(&mut dry, format()).map_err(|e| does_not_fit(e.to_string()))?;
    if &dry.first[54..62] != type_name(fat) {
        return Err(does_not_fit(format!(
            "its size gives {}",
            String::from_utf8_lossy(&dry.first[54..62]).trim_end()
        )));
    }
    let mut io = SectorIo::new(volume);
    fatfs::format_volume(&mut io, format()).map_err(|e| does_not_fit(e.to_string()))?;
    let hidden = u32::try_from(options.first_sector)
        .map_err(|_| Error::host("format", "FAT counts the sectors before it in 32 bits"))?;
    io.seek(SeekFrom::Start(HIDDEN_SECTORS_AT))?;
    io.write_all(&hidden.to_le_bytes())?;
    io.flush()?;
    io.sync().map_err(burnout)?;
    Ok(())
}

/// Format the whole of `volume`, which is one partition, as `fs`.
pub fn format<T: BlockTarget>(volume: T, fs: FileSystem, options: &Options<'_>) -> Result<()> {
    match fs {
        FileSystem::Fat12 => format_small_fat(volume, FatType::Fat12, options),
        FileSystem::Fat16 => format_small_fat(volume, FatType::Fat16, options),
        FileSystem::Fat32 => {
            let first_sector = u32::try_from(options.first_sector).map_err(|_| {
                Error::host("format", "FAT32 counts the sectors before it in 32 bits")
            })?;
            format_fat32(
                volume,
                &Fat32Options {
                    label: options.label,
                    serial: options.serial,
                    first_sector,
                },
            )
            .map_err(burnout)
        }
        FileSystem::Exfat => {
            // An empty tree: the volume holds no file.
            write_exfat(
                volume,
                &MemorySource::new(),
                &ExfatOptions {
                    label: options.label,
                    serial: options.serial,
                    first_sector: options.first_sector,
                },
            )
            .map(|_| ())
            .map_err(burnout)
        }
    }
}

/// Read the first sector of a volume, for a check after a format.
pub fn boot_sector<T: BlockTarget>(volume: &mut T) -> Result<Vec<u8>> {
    let mut sector = vec![0u8; volume.logical_sector_size() as usize];
    volume.seek(SeekFrom::Start(0))?;
    volume.read_exact(&mut sector)?;
    Ok(sector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use burnout_core::MemoryTarget;

    const MIB: u64 = 1024 * 1024;

    fn options() -> Options<'static> {
        Options {
            label: "Onyx Test",
            serial: 0x1234_5678,
            first_sector: 2048,
        }
    }

    fn formatted(fs: FileSystem, size: u64) -> Vec<u8> {
        let mut t = MemoryTarget::new(size, 512).unwrap();
        format(&mut t, fs, &options()).unwrap();
        boot_sector(&mut t).unwrap()
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn a_label_becomes_eleven_upper_case_bytes() {
        assert_eq!(&fat_label("Onyx Test"), b"ONYX TEST  ");
        assert_eq!(&fat_label("a.b/c"), b"A_B_C      ");
        assert_eq!(&fat_label("much too long a name"), b"MUCH TOO LO");
    }

    #[test]
    fn fat12_names_its_type_and_says_where_it_starts() {
        let b = formatted(FileSystem::Fat12, 8 * MIB);
        assert_eq!(&b[54..62], b"FAT12   ");
        assert_eq!(u32_at(&b, 28), 2048);
        assert_eq!(u32_at(&b, 39), 0x1234_5678);
        assert_eq!(&b[43..54], b"ONYX TEST  ");
        assert_eq!(&b[510..512], &[0x55, 0xAA]);
    }

    #[test]
    fn fat16_names_its_type_and_says_where_it_starts() {
        let b = formatted(FileSystem::Fat16, 256 * MIB);
        assert_eq!(&b[54..62], b"FAT16   ");
        assert_eq!(u32_at(&b, 28), 2048);
    }

    #[test]
    fn fat32_comes_from_burnout_and_says_where_it_starts() {
        let b = formatted(FileSystem::Fat32, 512 * MIB);
        assert_eq!(&b[82..90], b"FAT32   ");
        assert_eq!(u32_at(&b, 28), 2048);
    }

    #[test]
    fn exfat_comes_from_burnout() {
        let b = formatted(FileSystem::Exfat, 256 * MIB);
        assert_eq!(&b[3..11], b"EXFAT   ");
        // exFAT keeps the start of its partition in 64 bits at byte 64.
        assert_eq!(u64::from_le_bytes(b[64..72].try_into().unwrap()), 2048);
    }

    #[test]
    fn a_partition_too_large_for_fat12_is_refused_and_not_formatted_as_another() {
        let mut t = MemoryTarget::new(1024 * MIB, 512).unwrap();
        let e = format(&mut t, FileSystem::Fat12, &options()).unwrap_err();
        assert!(e.to_string().contains("gives FAT16"), "{e}");
        assert!(boot_sector(&mut t).unwrap().iter().all(|b| *b == 0));
    }

    #[test]
    fn a_partition_too_small_for_fat32_is_refused() {
        let mut t = MemoryTarget::new(8 * MIB, 512).unwrap();
        assert!(format(&mut t, FileSystem::Fat32, &options()).is_err());
    }
}
