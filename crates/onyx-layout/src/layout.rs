//! A whole new layout on a drive: one table, one partition, one file system.

use std::io::SeekFrom;

use burnout_core::{BlockTarget, Window};
use onyx_core::{Error, Result};

use crate::fs::{self, FileSystem};
use crate::table::{self, Extent, Identity, Kind, Table};

/// What the person chose for the new layout.
#[derive(Clone, Copy, Debug)]
pub struct Layout<'a> {
    pub table: Table,
    pub fs: FileSystem,
    pub label: &'a str,
    /// The source of the GUIDs, the disk signature, and the volume serial.
    pub seed: u64,
}

fn burnout(error: burnout_core::Error) -> Error {
    Error::host("format", error.to_string())
}

/// Write a table with one empty partition of `kind`, and remove the old
/// tables at both ends. A tool of the host writes the file system after it.
/// It gives the place of the partition.
pub fn write_table<T: BlockTarget>(
    target: &mut T,
    table: Table,
    kind: Kind,
    seed: u64,
) -> Result<Extent> {
    let sector = target.logical_sector_size();
    let id = Identity::from_seed(seed);
    let plan = table::plan(table, kind, target.length(), sector, id)?;
    for w in &plan.writes {
        target.seek(SeekFrom::Start(w.offset))?;
        target.write_all(&w.bytes)?;
    }
    target.flush()?;
    target.sync().map_err(burnout)?;
    Ok(plan.partition)
}

/// Write the table, then the file system in its partition. This erases what
/// the drive held. It gives the place of the partition.
pub fn write<T: BlockTarget>(target: &mut T, layout: &Layout<'_>) -> Result<Extent> {
    let sector = target.logical_sector_size();
    let partition = write_table(target, layout.table, layout.fs.kind(), layout.seed)?;
    let Extent { start, length } = partition;
    let window = Window::new(&mut *target, start, length).map_err(burnout)?;
    fs::format(
        window,
        layout.fs,
        &fs::Options {
            label: layout.label,
            serial: Identity::from_seed(layout.seed).disk_signature,
            first_sector: start / u64::from(sector),
        },
    )?;
    target.sync().map_err(burnout)?;
    Ok(partition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use burnout_core::MemoryTarget;
    use std::io::{Read, Seek, Write};

    const MIB: u64 = 1024 * 1024;

    fn read(t: &mut MemoryTarget, at: u64, n: usize) -> Vec<u8> {
        let mut out = vec![0u8; n];
        t.seek(SeekFrom::Start(at)).unwrap();
        t.read_exact(&mut out).unwrap();
        out
    }

    fn layout(table: Table, fs: FileSystem) -> Layout<'static> {
        Layout {
            table,
            fs,
            label: "ONYX",
            seed: 7,
        }
    }

    #[test]
    fn gpt_and_fat32_put_the_volume_at_one_mib() {
        let mut t = MemoryTarget::new(512 * MIB, 512).unwrap();
        // An old table at both ends, which the new layout must remove.
        t.seek(SeekFrom::Start(0)).unwrap();
        t.write_all(&[0xAB; 4096]).unwrap();
        t.seek(SeekFrom::End(-4096)).unwrap();
        t.write_all(&[0xAB; 4096]).unwrap();

        let part = write(&mut t, &layout(Table::Gpt, FileSystem::Fat32)).unwrap();
        assert_eq!(part.start, MIB);
        let mbr = read(&mut t, 0, 512);
        assert_eq!(mbr[450], 0xEE);
        assert_eq!(&read(&mut t, 512, 8), b"EFI PART");
        let boot = read(&mut t, MIB, 512);
        assert_eq!(&boot[82..90], b"FAT32   ");
        assert_eq!(u32::from_le_bytes(boot[28..32].try_into().unwrap()), 2048);
        assert!(
            read(&mut t, 512 * MIB - 4096, 3584)
                .iter()
                .all(|b| *b != 0xAB)
        );
    }

    #[test]
    fn mbr_and_exfat_name_the_partition_type_of_exfat() {
        let mut t = MemoryTarget::new(256 * MIB, 512).unwrap();
        let part = write(&mut t, &layout(Table::Mbr, FileSystem::Exfat)).unwrap();
        let mbr = read(&mut t, 0, 512);
        assert_eq!(mbr[450], 0x07);
        assert_eq!(&mbr[510..512], &[0x55, 0xAA]);
        assert_eq!(&read(&mut t, part.start + 3, 8), b"EXFAT   ");
    }

    #[test]
    fn mbr_and_fat16_put_the_volume_where_the_table_says() {
        let mut t = MemoryTarget::new(64 * MIB, 512).unwrap();
        write(&mut t, &layout(Table::Mbr, FileSystem::Fat16)).unwrap();
        let mbr = read(&mut t, 0, 512);
        let first = u32::from_le_bytes(mbr[454..458].try_into().unwrap());
        let boot = read(&mut t, u64::from(first) * 512, 512);
        assert_eq!(&boot[54..62], b"FAT16   ");
        assert_eq!(u32::from_le_bytes(boot[28..32].try_into().unwrap()), first);
    }

    #[test]
    fn a_table_alone_leaves_the_partition_empty_for_the_host_tool() {
        let mut t = MemoryTarget::new(64 * MIB, 512).unwrap();
        t.seek(SeekFrom::Start(MIB)).unwrap();
        t.write_all(&[0xAB; 512]).unwrap();
        let part = write_table(&mut t, Table::Mbr, Kind::Linux, 3).unwrap();
        assert_eq!(read(&mut t, 0, 512)[450], 0x83);
        // The old bytes in the partition stay. mkfs writes over them.
        assert_eq!(read(&mut t, part.start, 512), vec![0xAB; 512]);
    }

    #[test]
    fn a_file_system_that_does_not_fit_leaves_the_partition_unformatted() {
        let mut t = MemoryTarget::new(1024 * MIB, 512).unwrap();
        assert!(write(&mut t, &layout(Table::Mbr, FileSystem::Fat12)).is_err());
        assert!(read(&mut t, MIB, 512).iter().all(|b| *b == 0));
    }
}
