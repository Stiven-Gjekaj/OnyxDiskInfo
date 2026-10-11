//! What an ATA drive reports about itself.
//!
//! The layouts come from the ATA Command Set (ACS). Each reader takes the
//! bytes that the drive gave, and makes no system call.
//!
//! IDENTIFY DEVICE gives 256 words of 16 bits, little endian. A string in it
//! keeps two characters in each word, and the first of the two is in the high
//! byte. That swap is the common trap of this module.
//!
//! The SMART attribute table is not in ACS. Each maker defines its own
//! attributes. The layout of the table is the same on every drive, and the
//! meaning of each raw value is not. [`attribute_name`] gives the names that
//! most makers agree on.

use crate::{Error, Result};

/// The size of each ATA data block that this module reads, in bytes.
pub const BLOCK_BYTES: usize = 512;

/// The number of entries in the attribute table and in the threshold table.
const TABLE_ENTRIES: usize = 30;

/// The size of one entry, in bytes.
const ENTRY_BYTES: usize = 12;

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

/// One SMART attribute, with its threshold.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Attribute {
    pub id: u8,
    /// The flags. Bit 0 says that the attribute warns of a failure before it
    /// happens.
    pub flags: u16,
    /// The normalized value, from 1 to 253 on most drives. Higher is better.
    pub current: u8,
    /// The lowest normalized value that the drive has seen.
    pub worst: u8,
    /// The six raw bytes, as the drive stores them.
    pub raw: [u8; 6],
    /// The threshold. The attribute fails when `current` is at or below it.
    /// A threshold of zero never fails. `None` means the drive gave no
    /// threshold for this attribute.
    pub threshold: Option<u8>,
}

impl Attribute {
    /// Build an attribute from values that another tool decoded.
    ///
    /// The raw value keeps its lowest 48 bits, the six bytes of the table.
    pub fn new(
        id: u8,
        flags: u16,
        current: u8,
        worst: u8,
        raw: u64,
        threshold: Option<u8>,
    ) -> Self {
        let mut bytes = [0u8; 6];
        bytes.copy_from_slice(&raw.to_le_bytes()[..6]);
        Attribute {
            id,
            flags,
            current,
            worst,
            raw: bytes,
            threshold,
        }
    }

    /// The raw value as one number of 48 bits.
    ///
    /// This is the reading that `smartctl` shows by default. Some attributes
    /// pack more than one value into the six bytes. [`Attribute::value`]
    /// unpacks the common ones.
    pub fn raw48(&self) -> u64 {
        self.raw
            .iter()
            .rev()
            .fold(0u64, |value, b| value << 8 | u64::from(*b))
    }

    /// The attribute warns of a failure before it happens.
    pub fn pre_fail(&self) -> bool {
        self.flags & 1 != 0
    }

    /// The normalized value is at or below a threshold that is not zero.
    pub fn failing_now(&self) -> bool {
        matches!(self.threshold, Some(t) if t != 0 && self.current <= t)
    }

    /// The normalized value went at or below a threshold that is not zero at
    /// some time in the past.
    pub fn failed_in_past(&self) -> bool {
        matches!(self.threshold, Some(t) if t != 0 && self.worst <= t)
    }

    /// The value that a person reads.
    ///
    /// A temperature keeps the current reading in the lowest byte and a
    /// minimum and a maximum in higher bytes. Power-on time keeps the hours
    /// in the lowest four bytes on most drives. Every other attribute is the
    /// whole raw value.
    pub fn value(&self) -> u64 {
        match self.id {
            TEMPERATURE | AIRFLOW_TEMPERATURE => u64::from(self.raw[0]),
            POWER_ON_HOURS => self.raw48() & 0xffff_ffff,
            _ => self.raw48(),
        }
    }
}

/// The SMART data of one drive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Smart {
    /// The attributes, in the order of the table.
    pub attributes: Vec<Attribute>,
    /// The status of the last self test, from byte 363. The high four bits
    /// give the result, and 0 means the test passed or none ran.
    pub self_test_status: u8,
    /// The checksum in byte 511 is wrong.
    pub bad_checksum: bool,
}

impl Smart {
    /// The attribute with this identifier, if the drive has it.
    pub fn attribute(&self, id: u8) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.id == id)
    }

    /// The temperature in degrees Celsius, from attribute 194 or 190.
    pub fn temperature_celsius(&self) -> Option<i32> {
        [TEMPERATURE, AIRFLOW_TEMPERATURE]
            .iter()
            .find_map(|id| self.attribute(*id))
            .map(|a| i32::from(a.raw[0]))
            .filter(|t| *t > 0)
    }
}

fn check_block(bytes: &[u8], what: &str) -> Result<bool> {
    if bytes.len() < BLOCK_BYTES {
        return Err(Error::host(
            what,
            format!("{} bytes, and the block has {BLOCK_BYTES}", bytes.len()),
        ));
    }
    Ok(bytes[..BLOCK_BYTES]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_add(*b))
        != 0)
}

/// Read SMART READ DATA and SMART READ THRESHOLDS together.
///
/// An entry with an identifier of zero is empty. A threshold table that the
/// drive refused leaves each threshold absent.
pub fn smart(data: &[u8], thresholds: Option<&[u8]>) -> Result<Smart> {
    let bad_checksum = check_block(data, "ATA SMART READ DATA")?;
    let limits: Vec<(u8, u8)> = match thresholds {
        Some(t) => {
            check_block(t, "ATA SMART READ THRESHOLDS")?;
            (0..TABLE_ENTRIES)
                .map(|i| 2 + i * ENTRY_BYTES)
                .map(|at| (t[at], t[at + 1]))
                .filter(|(id, _)| *id != 0)
                .collect()
        }
        None => Vec::new(),
    };

    let attributes = (0..TABLE_ENTRIES)
        .map(|i| 2 + i * ENTRY_BYTES)
        .filter(|at| data[*at] != 0)
        .map(|at| {
            let id = data[at];
            let mut raw = [0u8; 6];
            raw.copy_from_slice(&data[at + 5..at + 11]);
            Attribute {
                id,
                flags: u16::from_le_bytes([data[at + 1], data[at + 2]]),
                current: data[at + 3],
                worst: data[at + 4],
                raw,
                threshold: limits.iter().find(|(t, _)| *t == id).map(|(_, v)| *v),
            }
        })
        .collect();

    Ok(Smart {
        attributes,
        self_test_status: data[363],
        bad_checksum,
    })
}

pub const READ_ERROR_RATE: u8 = 1;
pub const REALLOCATED_SECTORS: u8 = 5;
pub const POWER_ON_HOURS: u8 = 9;
pub const POWER_CYCLES: u8 = 12;
pub const AIRFLOW_TEMPERATURE: u8 = 190;
pub const TEMPERATURE: u8 = 194;
pub const PENDING_SECTORS: u8 = 197;
pub const OFFLINE_UNCORRECTABLE: u8 = 198;

/// The name that most makers give an attribute.
///
/// A maker can give an identifier its own meaning. These names are the ones
/// that most drives agree on. An identifier that is not here has no name.
pub fn attribute_name(id: u8) -> Option<&'static str> {
    Some(match id {
        1 => "Read Error Rate",
        2 => "Throughput Performance",
        3 => "Spin-Up Time",
        4 => "Start/Stop Count",
        5 => "Reallocated Sectors Count",
        7 => "Seek Error Rate",
        8 => "Seek Time Performance",
        9 => "Power-On Hours",
        10 => "Spin Retry Count",
        11 => "Recalibration Retries",
        12 => "Power Cycle Count",
        170 => "Available Reserved Space",
        171 => "Program Fail Count",
        172 => "Erase Fail Count",
        173 => "Wear Leveling Count",
        174 => "Unexpected Power Loss Count",
        177 => "Wear Range Delta",
        179 => "Used Reserved Block Count Total",
        181 => "Program Fail Count Total",
        182 => "Erase Fail Count Total",
        183 => "Runtime Bad Block",
        184 => "End-to-End Error",
        187 => "Reported Uncorrectable Errors",
        188 => "Command Timeout",
        189 => "High Fly Writes",
        190 => "Airflow Temperature",
        191 => "G-Sense Error Rate",
        192 => "Power-Off Retract Count",
        193 => "Load Cycle Count",
        194 => "Temperature",
        195 => "Hardware ECC Recovered",
        196 => "Reallocation Event Count",
        197 => "Current Pending Sector Count",
        198 => "Offline Uncorrectable Sector Count",
        199 => "UltraDMA CRC Error Count",
        200 => "Write Error Rate",
        202 => "Percent Lifetime Remaining",
        220 => "Disk Shift",
        222 => "Loaded Hours",
        223 => "Load/Unload Retry Count",
        224 => "Load Friction",
        225 => "Load/Unload Cycle Count",
        226 => "Load-in Time",
        230 => "GMR Head Amplitude",
        231 => "Life Left",
        232 => "Endurance Remaining",
        233 => "Media Wearout Indicator",
        234 => "Average Erase Count",
        235 => "Good Block Count",
        240 => "Head Flying Hours",
        // The unit is the maker's: sectors on most hard disks, GiB on a
        // Kingston SA400, 32 MiB on others. So the name gives none.
        241 => "Total Host Writes",
        242 => "Total Host Reads",
        254 => "Free Fall Protection",
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // Most blocks here are built to the layout in ACS, and the tests say
    // which field they check. A built block reaches the cases that a healthy
    // drive never shows, such as a failing attribute. The blocks of a real
    // Kingston drive come last, and they prove the layout against smartctl.

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

    /// One attribute entry: identifier, flags, current, worst, raw.
    fn put_attribute(block: &mut [u8], slot: usize, id: u8, flags: u16, cur: u8, raw: u64) {
        let at = 2 + slot * ENTRY_BYTES;
        block[at] = id;
        block[at + 1..at + 3].copy_from_slice(&flags.to_le_bytes());
        block[at + 3] = cur;
        block[at + 4] = cur;
        block[at + 5..at + 11].copy_from_slice(&raw.to_le_bytes()[..6]);
    }

    fn put_threshold(block: &mut [u8], slot: usize, id: u8, threshold: u8) {
        let at = 2 + slot * ENTRY_BYTES;
        block[at] = id;
        block[at + 1] = threshold;
    }

    /// Set the checksum in byte 511 so that the block adds up to zero.
    fn seal_data(block: &mut [u8]) {
        let sum = block[..511].iter().fold(0u8, |s, b| s.wrapping_add(*b));
        block[511] = 0u8.wrapping_sub(sum);
    }

    pub(crate) fn smart_blocks() -> (Vec<u8>, Vec<u8>) {
        let mut data = vec![0u8; BLOCK_BYTES];
        data[0] = 0x10;
        put_attribute(&mut data, 0, REALLOCATED_SECTORS, 0x33, 100, 0);
        put_attribute(&mut data, 1, POWER_ON_HOURS, 0x32, 95, 21_544);
        put_attribute(&mut data, 2, POWER_CYCLES, 0x32, 99, 1_212);
        // 36 now, a minimum of 20 and a maximum of 52.
        put_attribute(&mut data, 3, TEMPERATURE, 0x22, 64, 0x0034_0014_0024);
        put_attribute(&mut data, 4, PENDING_SECTORS, 0x32, 100, 0);
        seal_data(&mut data);

        let mut limits = vec![0u8; BLOCK_BYTES];
        put_threshold(&mut limits, 0, REALLOCATED_SECTORS, 10);
        put_threshold(&mut limits, 1, POWER_ON_HOURS, 0);
        put_threshold(&mut limits, 2, POWER_CYCLES, 0);
        put_threshold(&mut limits, 3, TEMPERATURE, 0);
        seal_data(&mut limits);
        (data, limits)
    }

    #[test]
    fn each_used_entry_of_the_table_becomes_an_attribute() {
        let (data, limits) = smart_blocks();
        let s = smart(&data, Some(&limits)).unwrap();
        let ids: Vec<u8> = s.attributes.iter().map(|a| a.id).collect();
        assert_eq!(ids, [5, 9, 12, 194, 197]);
        assert!(!s.bad_checksum);
    }

    #[test]
    fn an_attribute_takes_the_threshold_with_its_identifier() {
        let (data, limits) = smart_blocks();
        let s = smart(&data, Some(&limits)).unwrap();
        assert_eq!(s.attribute(5).unwrap().threshold, Some(10));
        assert_eq!(s.attribute(9).unwrap().threshold, Some(0));
        // The threshold table has no entry for 197.
        assert_eq!(s.attribute(197).unwrap().threshold, None);
    }

    #[test]
    fn with_no_threshold_table_each_threshold_is_absent() {
        let (data, _) = smart_blocks();
        let s = smart(&data, None).unwrap();
        assert!(s.attributes.iter().all(|a| a.threshold.is_none()));
    }

    #[test]
    fn an_attribute_built_from_values_keeps_48_bits_of_its_raw_value() {
        let a = Attribute::new(9, 0x32, 95, 95, 0x00ff_1122_3344_5566, Some(0));
        assert_eq!(a.raw48(), 0x1122_3344_5566);
        assert_eq!(a.raw, [0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn the_raw_value_is_six_bytes_little_endian() {
        let (data, limits) = smart_blocks();
        let s = smart(&data, Some(&limits)).unwrap();
        assert_eq!(s.attribute(9).unwrap().raw48(), 21_544);
        assert_eq!(s.attribute(194).unwrap().raw48(), 0x0034_0014_0024);
    }

    #[test]
    fn the_temperature_is_the_lowest_byte_of_its_raw_value() {
        let (data, limits) = smart_blocks();
        let s = smart(&data, Some(&limits)).unwrap();
        assert_eq!(s.attribute(194).unwrap().value(), 36);
        assert_eq!(s.temperature_celsius(), Some(36));
    }

    #[test]
    fn power_on_hours_keep_only_the_lowest_four_bytes() {
        let a = Attribute {
            id: POWER_ON_HOURS,
            flags: 0,
            current: 100,
            worst: 100,
            raw: [0x10, 0x27, 0, 0, 0x99, 0x01],
            threshold: None,
        };
        assert_eq!(a.value(), 10_000);
    }

    #[test]
    fn an_attribute_fails_at_its_threshold_and_not_above_it() {
        let mut a = Attribute {
            id: REALLOCATED_SECTORS,
            flags: 0x33,
            current: 11,
            worst: 11,
            raw: [0; 6],
            threshold: Some(10),
        };
        assert!(a.pre_fail());
        assert!(!a.failing_now());
        a.current = 10;
        assert!(a.failing_now());
        a.threshold = Some(0);
        assert!(!a.failing_now(), "a threshold of zero never fails");
        a.threshold = None;
        assert!(!a.failing_now());
    }

    #[test]
    fn a_worst_value_at_the_threshold_is_a_failure_in_the_past() {
        let a = Attribute {
            id: REALLOCATED_SECTORS,
            flags: 0x33,
            current: 100,
            worst: 9,
            raw: [0; 6],
            threshold: Some(10),
        };
        assert!(!a.failing_now());
        assert!(a.failed_in_past());
    }

    #[test]
    fn a_wrong_smart_checksum_is_reported() {
        let (mut data, limits) = smart_blocks();
        data[100] ^= 0xff;
        assert!(smart(&data, Some(&limits)).unwrap().bad_checksum);
    }

    #[test]
    fn a_smart_block_too_short_is_an_error_and_not_a_panic() {
        let (data, _) = smart_blocks();
        assert!(smart(&data[..511], None).is_err());
        assert!(smart(&data, Some(&[0u8; 10])).is_err());
    }

    #[test]
    fn the_common_attributes_have_names_and_the_others_do_not() {
        assert_eq!(attribute_name(5), Some("Reallocated Sectors Count"));
        assert_eq!(attribute_name(194), Some("Temperature"));
        assert_eq!(attribute_name(6), None);
        // A name that gives no unit, because each maker counts in its own.
        assert_eq!(attribute_name(241), Some("Total Host Writes"));
    }

    /// Parse a block that is written as lines of hexadecimal.
    fn from_hex(lines: &[&str]) -> Vec<u8> {
        let text: String = lines.concat();
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    // A KINGSTON SA400S37240G, firmware S3Z40107, in a VIA VL715 USB case.
    // Captured on 2026-10-11 with `smartctl -r ioctl,2 -i -A -d sat` on
    // Windows 11, through the same SAT bridge that Onyx read.

    /// IDENTIFY DEVICE. The serial number is replaced with
    /// `SERIALMASKED0001`, which has the same length. The world wide name
    /// in words 108 to 111 holds the serial number too, so it keeps the NAA
    /// and the OUI of Kingston and the rest is zero. The checksum in byte 511
    /// is made again for the new bytes.
    fn kingston_identify() -> Vec<u8> {
        from_hex(&[
            "4000ff3f37c81000000000003f00000000000000455349524c41414d4b534445",
            "30303130202020200000000000003353345a31303730494b474e54534e4f5320",
            "3441303033533237303420472020202020202020202020202020202020200180",
            "0040002f0040000000000700ff3f10003f0010fcfb000101ffffff0f00000700",
            "03007800780078007800000d000000000000000000001f000e8506004c004400",
            "f8071b016b7409746041697401b460417f40030003000000feff000000000000",
            "0000000000000000b044f21b0000000000000800004000000250776b00000000",
            "00000000000000000000000000001c401c400000000000000000000000000000",
            "2100000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000100000000000000000000000000",
            "2020202020202020202020202020202020202020202020202020202020202020",
            "2020202020202020202020202020202020202020202020202020202000000000",
            "00000040000000000000000000000000000001000000000000000000ff100000",
            "0000000000000000000000000000000000000000200020000000000000000000",
            "000000000000000000000000000000000000000000000000000000000000a5c2",
        ])
    }

    /// SMART READ DATA, as the drive gave it.
    fn kingston_smart_data() -> Vec<u8> {
        from_hex(&[
            "01000132006464640000000000000932006464170000000000000c320064641c",
            "000000000000940000646400000000000000950000646400000000000000a700",
            "00646400000000000000a81200646400000000000000a9000064640000000000",
            "0000aa0000646400000000000000ac3200646400000000000000ad0000646400",
            "000000000000b53200646400000000000000b60000646400000000000000bb32",
            "00646400000000000000c0120064640f000000000000c222002e4e2e004e001f",
            "0000c43200646400000000000000c73200646400000000000000da3200646400",
            "000000000000e70000646464000000000000e9320064644c020000000000f132",
            "006464f9010000000000f232006464db010000000000f4000064640800000000",
            "0000f5000064640d000000000000f600006464f50f0000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000007800001102000100020a00000000000000000000",
            "00005231323138413020303000000000534d4932323538585400000000000000",
            "0000000000000000000000000000380200000042313630300000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "fffff50f000008000d0004000000000000000000000000000000000000000037",
        ])
    }

    /// SMART READ THRESHOLDS, as the drive gave it.
    fn kingston_thresholds() -> Vec<u8> {
        from_hex(&[
            "01000100000000000000000000000900000000000000000000000c0000000000",
            "000000000000940000000000000000000000950000000000000000000000a700",
            "00000000000000000000a80000000000000000000000a9000000000000000000",
            "0000aa0a00000000000000000000ac0000000000000000000000ad0000000000",
            "000000000000b50000000000000000000000b60000000000000000000000bb00",
            "00000000000000000000c00000000000000000000000c2000000000000000000",
            "0000c40000000000000000000000c70000000000000000000000da0000000000",
            "000000000000e70000000000000000000000e90000000000000000000000f100",
            "00000000000000000000f20000000000000000000000f4000000000000000000",
            "0000f50000000000000000000000f60000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "000000000000000000000000000000000000000000000000000000000000001c",
        ])
    }

    #[test]
    fn the_identify_data_of_a_real_sata_drive_gives_what_smartctl_gave() {
        let id = identity(&kingston_identify()).unwrap();
        assert_eq!(id.model.as_deref(), Some("KINGSTON SA400S37240G"));
        assert_eq!(id.serial.as_deref(), Some("SERIALMASKED0001"));
        assert_eq!(id.firmware.as_deref(), Some("S3Z40107"));
        // smartctl: "User Capacity: 240,057,409,536 bytes", "Sector Size:
        // 512 bytes logical/physical", "Rotation Rate: Solid State Device".
        assert_eq!(id.size_bytes(), 240_057_409_536);
        assert_eq!(id.logical_sector_size, 512);
        assert_eq!(id.rotation_rate, Some(0));
        assert!(id.smart_supported && id.smart_enabled);
        assert!(!id.bad_checksum);
    }

    #[test]
    fn the_smart_data_of_a_real_sata_drive_gives_what_smartctl_gave() {
        let s = smart(&kingston_smart_data(), Some(&kingston_thresholds())).unwrap();
        assert!(!s.bad_checksum);
        // Each row of `smartctl -A`: the id, the value, the worst, the
        // threshold, and the raw value.
        let table: [(u8, u8, u8, u8, u64); 26] = [
            (1, 100, 100, 0, 100),
            (9, 100, 100, 0, 23),
            (12, 100, 100, 0, 28),
            (148, 100, 100, 0, 0),
            (149, 100, 100, 0, 0),
            (167, 100, 100, 0, 0),
            (168, 100, 100, 0, 0),
            (169, 100, 100, 0, 0),
            (170, 100, 100, 10, 0),
            (172, 100, 100, 0, 0),
            (173, 100, 100, 0, 0),
            (181, 100, 100, 0, 0),
            (182, 100, 100, 0, 0),
            (187, 100, 100, 0, 0),
            (192, 100, 100, 0, 15),
            (194, 46, 78, 0, 133_149_098_030),
            (196, 100, 100, 0, 0),
            (199, 100, 100, 0, 0),
            (218, 100, 100, 0, 0),
            (231, 100, 100, 0, 100),
            (233, 100, 100, 0, 588),
            (241, 100, 100, 0, 505),
            (242, 100, 100, 0, 475),
            (244, 100, 100, 0, 8),
            (245, 100, 100, 0, 13),
            (246, 100, 100, 0, 4085),
        ];
        assert_eq!(s.attributes.len(), table.len());
        for (a, (id, current, worst, threshold, raw)) in s.attributes.iter().zip(table) {
            assert_eq!(
                (a.id, a.current, a.worst, a.threshold, a.raw48()),
                (id, current, worst, Some(threshold), raw),
                "attribute {id}"
            );
            assert!(!a.failing_now() && !a.failed_in_past(), "attribute {id}");
        }
        // "46 (Min/Max 31/78)": the reading is the lowest byte.
        assert_eq!(s.temperature_celsius(), Some(46));
        assert_eq!(s.attribute(POWER_ON_HOURS).map(Attribute::value), Some(23));
        assert_eq!(s.attribute(POWER_CYCLES).map(Attribute::value), Some(28));
    }
}
