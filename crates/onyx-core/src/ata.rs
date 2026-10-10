//! What an ATA drive reports about itself.
//!
//! The layouts come from the ATA Command Set (ACS). Each reader takes the
//! bytes that the drive gave, and makes no system call.
//!
//! IDENTIFY DEVICE gives 256 words of 16 bits, little endian. A string in it
//! keeps two characters in each word, and the first of the two is in the high
//! byte. That swap is the common trap of this module.

use crate::{Error, Result};

/// The size of each ATA data block that this module reads, in bytes.
pub const BLOCK_BYTES: usize = 512;

/// The answer to IDENTIFY DEVICE, as far as Onyx reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Identity {
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub model: Option<String>,
    /// The drive has the SMART feature set.
    pub smart_supported: bool,
    /// The SMART feature set is on.
    pub smart_enabled: bool,
    /// The number of user sectors.
    pub sectors: u64,
    /// The size of one logical sector, in bytes.
    pub logical_sector_size: u32,
    /// The rotation rate in revolutions per minute. `Some(0)` is a solid
    /// state drive. `None` means the drive does not say.
    pub rotation_rate: Option<u16>,
    /// The checksum in word 255 is present and wrong.
    pub bad_checksum: bool,
}

impl Identity {
    /// The size of the drive in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.sectors
            .saturating_mul(u64::from(self.logical_sector_size))
    }
}

fn word(bytes: &[u8], index: usize) -> u16 {
    let at = index * 2;
    match bytes.get(at..at + 2) {
        Some(b) => u16::from_le_bytes([b[0], b[1]]),
        None => 0,
    }
}

/// Read a string of `words` words that starts at word `first`.
fn ata_string(bytes: &[u8], first: usize, words: usize) -> Option<String> {
    let mut text = Vec::with_capacity(words * 2);
    for index in first..first + words {
        let [low, high] = word(bytes, index).to_le_bytes();
        text.push(high);
        text.push(low);
    }
    let end = text
        .iter()
        .position(|b| !(b.is_ascii_graphic() || *b == b' '))
        .unwrap_or(text.len());
    let text = String::from_utf8_lossy(&text[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Read the answer to IDENTIFY DEVICE.
pub fn identity(bytes: &[u8]) -> Result<Identity> {
    if bytes.len() < BLOCK_BYTES {
        return Err(Error::host(
            "ATA IDENTIFY DEVICE",
            format!("{} bytes, and the answer has {BLOCK_BYTES}", bytes.len()),
        ));
    }
    let bit = |index: usize, bit: u16| word(bytes, index) & (1 << bit) != 0;

    // Words 100 to 103 hold the count when the drive has 48-bit addresses.
    // Words 60 and 61 hold it for a drive that has only 28-bit ones.
    let lba48 = bit(83, 10);
    let sectors = if lba48 {
        (0..4).fold(0u64, |count, i| {
            count | u64::from(word(bytes, 100 + i)) << (16 * i)
        })
    } else {
        u64::from(word(bytes, 60)) | u64::from(word(bytes, 61)) << 16
    };

    // Word 106 is valid when bit 14 is set and bit 15 is clear. Bit 12 then
    // says that words 117 and 118 give the logical sector size, in words.
    let w106 = word(bytes, 106);
    let logical_sector_size = if w106 & 0xc000 == 0x4000 && w106 & (1 << 12) != 0 {
        let words = u32::from(word(bytes, 117)) | u32::from(word(bytes, 118)) << 16;
        words.saturating_mul(2)
    } else {
        512
    };

    let rotation_rate = match word(bytes, 217) {
        0 | 0xffff => None,
        1 => Some(0),
        rpm => Some(rpm),
    };

    // Word 255: 0xA5 in the low byte means the high byte is a checksum, and
    // all 512 bytes then add up to zero.
    let w255 = word(bytes, 255);
    let bad_checksum = w255 & 0xff == 0xa5
        && bytes[..BLOCK_BYTES]
            .iter()
            .fold(0u8, |sum, b| sum.wrapping_add(*b))
            != 0;

    Ok(Identity {
        serial: ata_string(bytes, 10, 10),
        firmware: ata_string(bytes, 23, 4),
        model: ata_string(bytes, 27, 20),
        smart_supported: bit(82, 0),
        smart_enabled: bit(85, 0),
        sectors,
        logical_sector_size: if logical_sector_size == 0 {
            512
        } else {
            logical_sector_size
        },
        rotation_rate,
        bad_checksum,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // No SATA drive was at hand when this module was written. Each block
    // here is built to the layout in ACS, and the tests say which field they
    // check. A dump captured from a real drive replaces them when one is at
    // hand.

    /// Put a string into IDENTIFY DEVICE words, high byte first.
    fn put_string(block: &mut [u8], first_word: usize, words: usize, text: &str) {
        let mut padded = text.as_bytes().to_vec();
        padded.resize(words * 2, b' ');
        for (i, pair) in padded.chunks(2).enumerate() {
            let at = (first_word + i) * 2;
            block[at] = pair[1];
            block[at + 1] = pair[0];
        }
    }

    fn put_word(block: &mut [u8], index: usize, value: u16) {
        block[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }

    /// Set the checksum in word 255 so that the block adds up to zero.
    fn seal(block: &mut [u8]) {
        block[510] = 0xa5;
        let sum = block[..511].iter().fold(0u8, |s, b| s.wrapping_add(*b));
        block[511] = 0u8.wrapping_sub(sum);
    }

    /// A 1 TB solid state drive with 48-bit addresses and SMART on.
    pub(crate) fn ssd_identity() -> Vec<u8> {
        let mut b = vec![0u8; BLOCK_BYTES];
        put_string(&mut b, 10, 10, "S4EWNX0R123456");
        put_string(&mut b, 23, 4, "RVT04B6Q");
        put_string(&mut b, 27, 20, "Samsung SSD 860 EVO 1TB");
        put_word(&mut b, 82, 1); // SMART supported
        put_word(&mut b, 83, 1 << 10); // 48-bit addresses
        put_word(&mut b, 85, 1); // SMART enabled
        // 1,953,525,168 sectors of 512 bytes.
        put_word(&mut b, 100, 0x6db0);
        put_word(&mut b, 101, 0x7470);
        put_word(&mut b, 217, 1); // not rotating
        seal(&mut b);
        b
    }

    #[test]
    fn the_strings_come_out_with_the_bytes_of_each_word_swapped_back() {
        let id = identity(&ssd_identity()).unwrap();
        assert_eq!(id.serial.as_deref(), Some("S4EWNX0R123456"));
        assert_eq!(id.firmware.as_deref(), Some("RVT04B6Q"));
        assert_eq!(id.model.as_deref(), Some("Samsung SSD 860 EVO 1TB"));
    }

    #[test]
    fn the_size_comes_from_the_48_bit_count() {
        let id = identity(&ssd_identity()).unwrap();
        assert_eq!(id.sectors, 1_953_525_168);
        assert_eq!(id.logical_sector_size, 512);
        assert_eq!(id.size_bytes(), 1_000_204_886_016);
    }

    #[test]
    fn a_drive_with_only_28_bit_addresses_takes_its_count_from_words_60_and_61() {
        let mut b = ssd_identity();
        put_word(&mut b, 83, 0);
        put_word(&mut b, 60, 0x0000);
        put_word(&mut b, 61, 0x0100);
        seal(&mut b);
        assert_eq!(identity(&b).unwrap().sectors, 0x0100_0000);
    }

    #[test]
    fn a_drive_of_4096_byte_logical_sectors_says_so_in_words_117_and_118() {
        let mut b = ssd_identity();
        put_word(&mut b, 106, 0x4000 | 1 << 12);
        put_word(&mut b, 117, 2048);
        seal(&mut b);
        assert_eq!(identity(&b).unwrap().logical_sector_size, 4096);
    }

    #[test]
    fn the_smart_bits_and_the_rotation_rate_are_read() {
        let id = identity(&ssd_identity()).unwrap();
        assert!(id.smart_supported && id.smart_enabled);
        assert_eq!(id.rotation_rate, Some(0));
        let mut b = ssd_identity();
        put_word(&mut b, 217, 7200);
        put_word(&mut b, 85, 0);
        seal(&mut b);
        let id = identity(&b).unwrap();
        assert_eq!(id.rotation_rate, Some(7200));
        assert!(!id.smart_enabled);
    }

    #[test]
    fn a_wrong_identify_checksum_is_reported_and_a_right_one_is_not() {
        assert!(!identity(&ssd_identity()).unwrap().bad_checksum);
        let mut b = ssd_identity();
        b[0] ^= 1;
        assert!(identity(&b).unwrap().bad_checksum);
        // A drive that does not use the checksum puts no 0xA5 there.
        b[510] = 0;
        assert!(!identity(&b).unwrap().bad_checksum);
    }

    #[test]
    fn an_identify_block_too_short_is_an_error_and_not_a_panic() {
        assert!(identity(&[0u8; 511]).is_err());
    }
}
