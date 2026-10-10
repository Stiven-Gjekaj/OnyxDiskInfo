//! The partition table of a drive with one partition: GPT or MBR.
//!
//! The partition starts at 1 MiB, as every host puts its first partition
//! today, so it is aligned for each sector size and each flash page. It runs
//! to the end of the usable space.
//!
//! A new table also removes the old one. A GPT keeps a copy of itself at the
//! end of the drive, and a host that finds that copy under a new MBR shows
//! the old partitions. So each plan writes zeros over the space of both
//! tables at both ends first.

use onyx_core::{Error, Result};

use crate::crc32::crc32;

/// The kind of partition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Gpt,
    Mbr,
}

/// What the partition holds, as the table names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fat12,
    Fat16,
    Fat32,
    /// exFAT and NTFS share their type in both tables.
    Exfat,
    Ntfs,
    Linux,
    Apfs,
    HfsPlus,
}

/// A GUID, as GPT stores it: the first three fields little endian, the last
/// two big endian.
fn guid(text: &str) -> [u8; 16] {
    let hex: Vec<u8> = text
        .bytes()
        .filter(u8::is_ascii_hexdigit)
        .collect::<Vec<u8>>()
        .chunks(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).unwrap_or("00");
            u8::from_str_radix(digits, 16).unwrap_or(0)
        })
        .collect();
    let mut out = [0u8; 16];
    if hex.len() == 16 {
        out[0..4].copy_from_slice(&[hex[3], hex[2], hex[1], hex[0]]);
        out[4..6].copy_from_slice(&[hex[5], hex[4]]);
        out[6..8].copy_from_slice(&[hex[7], hex[6]]);
        out[8..16].copy_from_slice(&hex[8..16]);
    }
    out
}

impl Kind {
    /// The type GUID of the partition in a GPT.
    pub fn gpt_type(self) -> [u8; 16] {
        guid(match self {
            // Microsoft basic data, which each FAT, exFAT and NTFS uses.
            Kind::Fat12 | Kind::Fat16 | Kind::Fat32 | Kind::Exfat | Kind::Ntfs => {
                "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"
            }
            Kind::Linux => "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
            Kind::Apfs => "7C3457EF-0000-11AA-AA11-00306543ECAC",
            Kind::HfsPlus => "48465300-0000-11AA-AA11-00306543ECAC",
        })
    }

    /// The type byte of the partition in an MBR. APFS has none, because an
    /// APFS drive is always GPT.
    pub fn mbr_type(self) -> Option<u8> {
        match self {
            Kind::Fat12 => Some(0x01),
            Kind::Fat16 => Some(0x0E),
            Kind::Fat32 => Some(0x0C),
            Kind::Exfat | Kind::Ntfs => Some(0x07),
            Kind::Linux => Some(0x83),
            Kind::HfsPlus => Some(0xAF),
            Kind::Apfs => None,
        }
    }
}

/// Where the one partition sits, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub start: u64,
    pub length: u64,
}

/// One write of a plan: bytes at an offset of the drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// What a table writes, and where its partition is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub partition: Extent,
    /// In order. The zeros come first, then the tables.
    pub writes: Vec<Write>,
}

/// The start of the partition: 1 MiB.
pub const START: u64 = 1024 * 1024;

/// The entries of a GPT: 128 of 128 bytes, which every host expects.
const ENTRIES: u64 = 128;
const ENTRY_BYTES: u64 = 128;
const ENTRY_ARRAY_BYTES: u64 = ENTRIES * ENTRY_BYTES;

/// The values that make one drive different from another: the GUIDs of a
/// GPT and the signature of an MBR. The caller gives them, so that a test
/// gives the same ones each time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub disk_guid: [u8; 16],
    pub partition_guid: [u8; 16],
    pub disk_signature: u32,
}

impl Identity {
    /// Values from a seed, such as the time. A GUID made this way is of
    /// version 4, random, as the standard allows.
    pub fn from_seed(seed: u64) -> Self {
        let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut make = || {
            let mut g = [0u8; 16];
            g[0..8].copy_from_slice(&next().to_le_bytes());
            g[8..16].copy_from_slice(&next().to_le_bytes());
            g[7] = (g[7] & 0x0f) | 0x40; // version 4, in the third field
            g[8] = (g[8] & 0x3f) | 0x80; // the variant of RFC 4122
            g
        };
        let disk_guid = make();
        let partition_guid = make();
        Identity {
            disk_guid,
            partition_guid,
            disk_signature: (next() as u32) | 1,
        }
    }
}

fn too_small(size: u64) -> Error {
    Error::host(
        "partition table",
        format!("a drive of {size} bytes has no room for a partition after 1 MiB"),
    )
}

/// The zeros that remove an old table: the first MiB, and the last 1 MiB
/// or what the drive has of it.
fn wipes(size: u64) -> Vec<Write> {
    let tail = START.min(size.saturating_sub(START));
    let mut out = vec![Write {
        offset: 0,
        bytes: vec![0; START.min(size) as usize],
    }];
    if tail > 0 {
        out.push(Write {
            offset: size - tail,
            bytes: vec![0; tail as usize],
        });
    }
    out
}

/// One MBR entry of a partition that starts and ends past the reach of CHS
/// addresses. Each host reads the LBA fields, and the CHS fields hold the
/// value that says "past the end", as each tool writes them.
fn mbr_entry(kind: u8, first: u64, sectors: u64) -> [u8; 16] {
    let mut e = [0u8; 16];
    e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[4] = kind;
    e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[8..12].copy_from_slice(&(first.min(u64::from(u32::MAX)) as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors.min(u64::from(u32::MAX)) as u32).to_le_bytes());
    e
}

fn mbr_sector(entry: [u8; 16], signature: u32, sector: u32) -> Vec<u8> {
    let mut mbr = vec![0u8; sector as usize];
    mbr[440..444].copy_from_slice(&signature.to_le_bytes());
    mbr[446..462].copy_from_slice(&entry);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

/// The plan of a table with one partition of `kind` on a drive of `size`
/// bytes and sectors of `sector` bytes.
pub fn plan(table: Table, kind: Kind, size: u64, sector: u32, id: Identity) -> Result<Plan> {
    let s = u64::from(sector);
    if s == 0 || !s.is_power_of_two() || s < 512 {
        return Err(Error::host(
            "partition table",
            format!("a sector of {sector} bytes"),
        ));
    }
    let sectors = size / s;
    match table {
        Table::Mbr => {
            let kind_byte = kind.mbr_type().ok_or_else(|| {
                Error::host(
                    "partition table",
                    "APFS needs a GPT, and an MBR has no type for it",
                )
            })?;
            let first = START / s;
            // An MBR counts sectors in 32 bits, so a larger drive keeps the
            // rest outside the partition.
            let count = sectors.saturating_sub(first).min(u64::from(u32::MAX));
            if count == 0 {
                return Err(too_small(size));
            }
            let mut writes = wipes(size);
            writes.push(Write {
                offset: 0,
                bytes: mbr_sector(
                    mbr_entry(kind_byte, first, count),
                    id.disk_signature,
                    sector,
                ),
            });
            Ok(Plan {
                partition: Extent {
                    start: first * s,
                    length: count * s,
                },
                writes,
            })
        }
        Table::Gpt => {
            let entry_sectors = ENTRY_ARRAY_BYTES.div_ceil(s);
            let last_lba = sectors.checked_sub(1).ok_or_else(|| too_small(size))?;
            let first_usable = 2 + entry_sectors;
            let last_usable = last_lba
                .checked_sub(1 + entry_sectors)
                .ok_or_else(|| too_small(size))?;
            let first = START / s;
            if first > last_usable {
                return Err(too_small(size));
            }
            // The partition ends on a whole MiB, as each host makes it.
            let per_mib = START / s;
            let end = (last_usable + 1) / per_mib * per_mib;
            if end <= first {
                return Err(too_small(size));
            }
            let last = end - 1;

            let mut entries = vec![0u8; ENTRY_ARRAY_BYTES as usize];
            entries[0..16].copy_from_slice(&kind.gpt_type());
            entries[16..32].copy_from_slice(&id.partition_guid);
            entries[32..40].copy_from_slice(&first.to_le_bytes());
            entries[40..48].copy_from_slice(&last.to_le_bytes());
            let entries_crc = crc32(&entries);

            let header = |current: u64, backup: u64, array: u64| {
                let mut h = vec![0u8; s as usize];
                h[0..8].copy_from_slice(b"EFI PART");
                h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
                h[12..16].copy_from_slice(&92u32.to_le_bytes());
                h[24..32].copy_from_slice(&current.to_le_bytes());
                h[32..40].copy_from_slice(&backup.to_le_bytes());
                h[40..48].copy_from_slice(&first_usable.to_le_bytes());
                h[48..56].copy_from_slice(&last_usable.to_le_bytes());
                h[56..72].copy_from_slice(&id.disk_guid);
                h[72..80].copy_from_slice(&array.to_le_bytes());
                h[80..84].copy_from_slice(&(ENTRIES as u32).to_le_bytes());
                h[84..88].copy_from_slice(&(ENTRY_BYTES as u32).to_le_bytes());
                h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
                let header_crc = crc32(&h[0..92]);
                h[16..20].copy_from_slice(&header_crc.to_le_bytes());
                h
            };

            // The protective MBR: one entry of type 0xEE that covers the
            // drive, so a tool that knows no GPT leaves the drive alone.
            let protective = mbr_entry(0xEE, 1, last_lba);
            let mut writes = wipes(size);
            writes.push(Write {
                offset: 0,
                bytes: mbr_sector(protective, 0, sector),
            });
            writes.push(Write {
                offset: s,
                bytes: header(1, last_lba, 2),
            });
            writes.push(Write {
                offset: 2 * s,
                bytes: entries.clone(),
            });
            let backup_array = last_lba - entry_sectors;
            writes.push(Write {
                offset: backup_array * s,
                bytes: entries,
            });
            writes.push(Write {
                offset: last_lba * s,
                bytes: header(last_lba, 1, backup_array),
            });
            Ok(Plan {
                partition: Extent {
                    start: first * s,
                    length: (last + 1 - first) * s,
                },
                writes,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn id() -> Identity {
        Identity::from_seed(7)
    }

    /// Apply a plan to a drive in memory.
    fn apply(plan: &Plan, size: u64) -> Vec<u8> {
        let mut drive = vec![0xAAu8; size as usize];
        for w in &plan.writes {
            let at = w.offset as usize;
            drive[at..at + w.bytes.len()].copy_from_slice(&w.bytes);
        }
        drive
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    #[test]
    fn a_guid_keeps_its_first_three_fields_little_endian() {
        let g = guid("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7");
        assert_eq!(
            g,
            [
                0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26,
                0x99, 0xC7
            ]
        );
    }

    #[test]
    fn an_mbr_holds_one_partition_from_1_mib_to_the_end() {
        let size = 2 * GIB;
        let p = plan(Table::Mbr, Kind::Fat32, size, 512, id()).unwrap();
        assert_eq!(
            p.partition,
            Extent {
                start: START,
                length: size - START
            }
        );
        let d = apply(&p, size);
        assert_eq!(&d[510..512], &[0x55, 0xAA]);
        assert_eq!(d[446 + 4], 0x0C);
        assert_eq!(u32_at(&d, 446 + 8), 2048);
        assert_eq!(u64::from(u32_at(&d, 446 + 12)), (size - START) / 512);
        assert_eq!(u32_at(&d, 440), id().disk_signature);
        // The second entry is empty.
        assert!(d[462..478].iter().all(|b| *b == 0));
    }

    #[test]
    fn an_mbr_removes_the_copy_of_an_old_gpt_at_the_end() {
        let size = 2 * GIB;
        let d = apply(
            &plan(Table::Mbr, Kind::Exfat, size, 512, id()).unwrap(),
            size,
        );
        assert!(d[(size - START) as usize..].iter().all(|b| *b == 0));
        assert!(d[512..START as usize].iter().all(|b| *b == 0));
    }

    #[test]
    fn an_mbr_on_a_drive_past_2_tib_stops_its_partition_at_the_limit() {
        let size = 3 * 1024 * GIB;
        let p = plan(Table::Mbr, Kind::Ntfs, size, 512, Identity::from_seed(1));
        // `apply` would need the whole drive in memory, so this test checks
        // the extent and the size of the writes only.
        let p = p.unwrap();
        assert_eq!(p.partition.length, u64::from(u32::MAX) * 512);
        assert!(p.writes.iter().all(|w| w.bytes.len() <= START as usize));
    }

    #[test]
    fn apfs_has_no_mbr_type() {
        assert!(plan(Table::Mbr, Kind::Apfs, GIB, 512, id()).is_err());
    }

    fn check_gpt(size: u64, sector: u32) {
        let s = u64::from(sector);
        let p = plan(Table::Gpt, Kind::Exfat, size, sector, id()).unwrap();
        let d = apply(&p, size);
        let last = size / s - 1;

        // The protective MBR.
        assert_eq!(d[446 + 4], 0xEE);
        assert_eq!(u32_at(&d, 446 + 8), 1);
        assert_eq!(&d[510..512], &[0x55, 0xAA]);

        for (at, current, backup) in [(s, 1, last), (last * s, last, 1)] {
            let h = &d[at as usize..(at + s) as usize];
            assert_eq!(&h[0..8], b"EFI PART");
            let mut zeroed = h[0..92].to_vec();
            zeroed[16..20].fill(0);
            assert_eq!(u32_at(h, 16), crc32(&zeroed), "the CRC of the header");
            assert_eq!(u64_at(h, 24), current);
            assert_eq!(u64_at(h, 32), backup);
            let array = u64_at(h, 72);
            let entries = &d[(array * s) as usize..(array * s + ENTRY_ARRAY_BYTES) as usize];
            assert_eq!(u32_at(h, 88), crc32(entries), "the CRC of the entries");
            assert_eq!(&entries[0..16], &Kind::Exfat.gpt_type());
            let first = u64_at(entries, 32);
            let end = u64_at(entries, 40);
            assert!(
                first >= u64_at(h, 40) && end <= u64_at(h, 48),
                "inside the usable space"
            );
            assert_eq!(first * s, START);
            assert_eq!((end + 1) * s % START, 0, "the end is on a whole MiB");
        }
        assert_eq!(p.partition.start, START);
    }

    #[test]
    fn a_gpt_has_two_headers_whose_checksums_and_bounds_hold() {
        check_gpt(2 * GIB, 512);
        check_gpt(2 * GIB, 4096);
        check_gpt(64 * 1024 * 1024 + 4096 * 3, 512);
    }

    #[test]
    fn a_drive_too_small_for_a_partition_after_1_mib_is_refused() {
        assert!(plan(Table::Gpt, Kind::Fat32, START, 512, id()).is_err());
        assert!(plan(Table::Mbr, Kind::Fat32, START, 512, id()).is_err());
        assert!(plan(Table::Gpt, Kind::Fat32, GIB, 100, id()).is_err());
    }

    #[test]
    fn a_seed_gives_guids_of_version_4_and_a_signature_that_is_not_zero() {
        let a = Identity::from_seed(1);
        let b = Identity::from_seed(2);
        assert_ne!(a.disk_guid, b.disk_guid);
        assert_ne!(a.disk_guid, a.partition_guid);
        assert_eq!(a.disk_guid[7] >> 4, 4);
        assert_eq!(a.disk_guid[8] >> 6, 0b10);
        assert_ne!(a.disk_signature, 0);
    }
}
