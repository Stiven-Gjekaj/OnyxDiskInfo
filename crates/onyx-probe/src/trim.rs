//! A full TRIM: tell a flash drive that no block holds data.
//!
//! A drive that takes the TRIM erases its blocks when it wants to, and most
//! drives read zeros from a trimmed block at once. A drive does not have to:
//! the standard lets it give back the old bytes or other bytes until it
//! erases the block. So Onyx reads a sample after the TRIM and says what it
//! found, and it does not call a TRIM an erase that a person can prove.
//!
//! This file decides the ranges and builds the requests. Each host sends
//! them from a shim of its own.

use std::fs::File;

use onyx_core::{Error, Result};

/// The most bytes in one request. A request for a whole drive can take
/// minutes, and a person sees no progress during it.
pub const CHUNK_BYTES: u64 = 1 << 30;

/// The ranges of a TRIM of a whole device: each offset and length, in whole
/// sectors, at most [`CHUNK_BYTES`] each.
pub fn ranges(size: u64, sector: u32) -> Vec<(u64, u64)> {
    let sector = u64::from(sector.max(1));
    let end = size / sector * sector;
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < end {
        let length = (end - offset).min(CHUNK_BYTES);
        out.push((offset, length));
        offset += length;
    }
    out
}

/// `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES` on Windows.
pub const IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES: u32 = 0x002D_9404;

/// The size of `DEVICE_MANAGE_DATA_SET_ATTRIBUTES`: seven fields of 32 bits.
const DSM_HEADER_BYTES: usize = 28;

/// The input of a TRIM of one range on Windows: the header, with the action
/// `DeviceDsmAction_Trim`, then one `DEVICE_DATA_SET_RANGE` at the next
/// multiple of 8.
pub fn windows_trim_request(offset: u64, length: u64) -> Vec<u8> {
    let ranges_at = DSM_HEADER_BYTES.next_multiple_of(8);
    let mut out = vec![0u8; ranges_at + 16];
    let mut put = |at: usize, value: u32| out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(0, DSM_HEADER_BYTES as u32); // Size
    put(4, 1); // Action: DeviceDsmAction_Trim
    put(8, 0); // Flags
    put(12, 0); // ParameterBlockOffset
    put(16, 0); // ParameterBlockLength
    put(20, ranges_at as u32); // DataSetRangesOffset
    put(24, 16); // DataSetRangesLength
    out[ranges_at..ranges_at + 8].copy_from_slice(&offset.to_le_bytes());
    out[ranges_at + 8..ranges_at + 16].copy_from_slice(&length.to_le_bytes());
    out
}

/// TRIM every range of a drive that is open for a write. `progress` hears
/// the bytes done after each request.
pub fn trim(file: &File, size: u64, sector: u32, mut progress: impl FnMut(u64)) -> Result<()> {
    // Windows gives a drive no file type, so only the other hosts ask.
    if !cfg!(windows) && file.metadata()?.file_type().is_file() {
        return Err(Error::host(
            "trim",
            "a TRIM goes to a drive, and this is a file",
        ));
    }
    for (offset, length) in ranges(size, sector) {
        send(file, offset, length)?;
        progress(offset + length);
    }
    Ok(())
}

fn send(file: &File, offset: u64, length: u64) -> Result<()> {
    #[cfg(target_os = "linux")]
    let sent = crate::linux::trim::discard(file, offset, length);
    #[cfg(target_os = "macos")]
    let sent = crate::macos::trim::unmap(file, offset, length);
    #[cfg(windows)]
    let sent = crate::windows::trim::trim(file, offset, length);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let sent: std::io::Result<()> = {
        let _ = (file, offset, length);
        Err(std::io::Error::other("this host has no TRIM"))
    };
    sent.map_err(|e| {
        Error::host(
            "trim",
            format!("the drive refused the TRIM at byte {offset}: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ranges_cover_the_whole_sectors_in_chunks() {
        let size = 2 * CHUNK_BYTES + 4096 + 100;
        let r = ranges(size, 512);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], (0, CHUNK_BYTES));
        assert_eq!(r[1], (CHUNK_BYTES, CHUNK_BYTES));
        // The 100 bytes past the last whole sector are no sector.
        assert_eq!(r[2], (2 * CHUNK_BYTES, 4096));
        assert!(ranges(511, 512).is_empty());
    }

    #[test]
    fn the_windows_request_has_the_layout_of_the_sdk() {
        let r = windows_trim_request(0x1000, 0x2000);
        assert_eq!(r.len(), 48);
        assert_eq!(&r[0..4], &28u32.to_le_bytes());
        assert_eq!(&r[4..8], &1u32.to_le_bytes());
        assert_eq!(&r[20..24], &32u32.to_le_bytes());
        assert_eq!(&r[24..28], &16u32.to_le_bytes());
        assert_eq!(&r[32..40], &0x1000u64.to_le_bytes());
        assert_eq!(&r[40..48], &0x2000u64.to_le_bytes());
    }

    #[test]
    fn a_file_is_refused() {
        let path = std::env::temp_dir().join(format!("onyx-trim-{}", std::process::id()));
        let file = File::create(&path).unwrap();
        let e = trim(&file, 4096, 512, |_| {}).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        if !cfg!(windows) {
            assert!(e.to_string().contains("this is a file"), "{e}");
        }
    }
}
