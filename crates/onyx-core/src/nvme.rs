//! What an NVMe drive reports about itself.
//!
//! The layouts come from the NVM Express Base Specification. Each reader
//! takes the bytes that the drive gave, and makes no system call.
//!
//! A drive can give fewer bytes than the specification asks for. A field
//! past the end of the buffer reads as absent, and the reader does not panic.

use crate::{Error, Result};

/// The size of the SMART / Health Information log page, in bytes.
pub const HEALTH_LOG_BYTES: usize = 512;

/// The log page identifier of the SMART / Health Information log.
pub const HEALTH_LOG_ID: u8 = 0x02;

/// The bits of the critical warning byte.
pub mod critical {
    /// The available spare is below its threshold.
    pub const SPARE: u8 = 1 << 0;
    /// A temperature is above or below a threshold.
    pub const TEMPERATURE: u8 = 1 << 1;
    /// A media error or an internal error makes the drive less reliable.
    pub const RELIABILITY: u8 = 1 << 2;
    /// The media is read only.
    pub const READ_ONLY: u8 = 1 << 3;
    /// The volatile memory backup device failed.
    pub const BACKUP: u8 = 1 << 4;
    /// The persistent memory region is read only.
    pub const PERSISTENT_MEMORY: u8 = 1 << 5;
}

/// The SMART / Health Information log, log page 0x02.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HealthLog {
    /// The critical warning byte. [`critical`] names its bits.
    pub critical_warning: u8,
    /// The composite temperature, in kelvin. Zero means the drive gives none.
    pub temperature_kelvin: u16,
    /// The spare capacity that is left, as a percentage.
    pub available_spare: u8,
    /// The percentage below which the drive sets [`critical::SPARE`].
    pub available_spare_threshold: u8,
    /// The part of the rated life that the drive has used, as a percentage.
    ///
    /// The value can pass 100. A drive past its rated life still runs.
    pub percentage_used: u8,
    /// Data read, in units of 1000 sectors of 512 bytes.
    pub data_units_read: u128,
    /// Data written, in units of 1000 sectors of 512 bytes.
    pub data_units_written: u128,
    pub host_read_commands: u128,
    pub host_write_commands: u128,
    /// The time that the controller was busy, in minutes.
    pub controller_busy_minutes: u128,
    pub power_cycles: u128,
    pub power_on_hours: u128,
    pub unsafe_shutdowns: u128,
    pub media_errors: u128,
    pub error_log_entries: u128,
    /// The time above the warning temperature, in minutes.
    pub warning_temperature_minutes: u32,
    /// The time above the critical temperature, in minutes.
    pub critical_temperature_minutes: u32,
    /// Temperature sensors 1 to 8, in kelvin. A sensor that the drive does
    /// not have is absent.
    pub sensors_kelvin: [Option<u16>; 8],
}

/// The number of bytes in one data unit.
const DATA_UNIT_BYTES: u128 = 512 * 1000;

impl HealthLog {
    /// The composite temperature in degrees Celsius, if the drive gives one.
    pub fn temperature_celsius(&self) -> Option<i32> {
        kelvin_to_celsius(self.temperature_kelvin)
    }

    /// The bytes read, from the count of data units.
    pub fn bytes_read(&self) -> u128 {
        self.data_units_read.saturating_mul(DATA_UNIT_BYTES)
    }

    /// The bytes written, from the count of data units.
    pub fn bytes_written(&self) -> u128 {
        self.data_units_written.saturating_mul(DATA_UNIT_BYTES)
    }
}

/// A temperature in kelvin as degrees Celsius. Zero kelvin means no value.
///
/// The specification rounds to whole kelvin, and 273 is the offset that it
/// uses.
pub fn kelvin_to_celsius(kelvin: u16) -> Option<i32> {
    (kelvin != 0).then(|| i32::from(kelvin) - 273)
}

/// The size of the Identify Controller data structure, in bytes.
pub const IDENTIFY_BYTES: usize = 4096;

/// The Identify Controller data structure, as far as Onyx reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Controller {
    /// The PCI vendor identifier.
    pub vendor_id: u16,
    /// The PCI subsystem vendor identifier.
    pub subsystem_vendor_id: u16,
    pub serial: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    /// The IEEE OUI of the maker.
    pub ieee_oui: u32,
    pub controller_id: u16,
    /// The version of the specification, as `(major, minor, tertiary)`.
    ///
    /// A controller older than version 1.2 gives zero here.
    pub version: Option<(u16, u8, u8)>,
    /// The warning composite temperature threshold, in kelvin.
    pub warning_temperature_kelvin: Option<u16>,
    /// The critical composite temperature threshold, in kelvin.
    pub critical_temperature_kelvin: Option<u16>,
    /// The total capacity of the NVM, in bytes, when the drive gives it.
    pub total_capacity: Option<u128>,
    pub namespaces: u32,
}

/// Read an ASCII field that the drive pads with spaces.
///
/// A byte that is not printable ASCII ends the field, so a drive that fills
/// the field with zeros or with noise does not put noise in a name.
fn text_at(bytes: &[u8], offset: usize, length: usize) -> Option<String> {
    let field = bytes.get(offset..offset + length)?;
    let end = field
        .iter()
        .position(|b| !(b.is_ascii_graphic() || *b == b' '))
        .unwrap_or(field.len());
    let text = String::from_utf8_lossy(&field[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Read the Identify Controller data structure.
///
/// The fields that Onyx reads end at byte 520. A shorter buffer is an error.
pub fn controller(bytes: &[u8]) -> Result<Controller> {
    const READ: usize = 520;
    if bytes.len() < READ {
        return Err(Error::host(
            "NVMe identify controller",
            format!("{} bytes, and Onyx reads {READ}", bytes.len()),
        ));
    }
    let oui = u32::from(byte_at(bytes, 73))
        | u32::from(byte_at(bytes, 74)) << 8
        | u32::from(byte_at(bytes, 75)) << 16;
    let version = u32_at(bytes, 80);
    let kelvin = |offset| Some(u16_at(bytes, offset)).filter(|k| *k != 0);
    Ok(Controller {
        vendor_id: u16_at(bytes, 0),
        subsystem_vendor_id: u16_at(bytes, 2),
        serial: text_at(bytes, 4, 20),
        model: text_at(bytes, 24, 40),
        firmware: text_at(bytes, 64, 8),
        ieee_oui: oui,
        controller_id: u16_at(bytes, 78),
        version: (version != 0).then_some((
            (version >> 16) as u16,
            (version >> 8) as u8,
            version as u8,
        )),
        warning_temperature_kelvin: kelvin(266),
        critical_temperature_kelvin: kelvin(268),
        total_capacity: Some(u128_at(bytes, 280)).filter(|c| *c != 0),
        namespaces: u32_at(bytes, 516),
    })
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    let mut word = [0u8; 2];
    if let Some(b) = bytes.get(offset..offset + 2) {
        word.copy_from_slice(b);
    }
    u16::from_le_bytes(word)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut word = [0u8; 4];
    if let Some(b) = bytes.get(offset..offset + 4) {
        word.copy_from_slice(b);
    }
    u32::from_le_bytes(word)
}

fn u128_at(bytes: &[u8], offset: usize) -> u128 {
    let mut word = [0u8; 16];
    if let Some(b) = bytes.get(offset..offset + 16) {
        word.copy_from_slice(b);
    }
    u128::from_le_bytes(word)
}

fn byte_at(bytes: &[u8], offset: usize) -> u8 {
    bytes.get(offset).copied().unwrap_or(0)
}

/// Read the SMART / Health Information log.
///
/// A buffer shorter than the fields that every drive fills is an error. The
/// fields after the first 216 bytes are newer, and a short buffer leaves
/// them at zero.
pub fn health_log(bytes: &[u8]) -> Result<HealthLog> {
    // Up to and including the eight temperature sensors.
    const FILLED: usize = 216;
    if bytes.len() < FILLED {
        return Err(Error::host(
            "NVMe health log",
            format!("{} bytes, and the log needs {FILLED}", bytes.len()),
        ));
    }
    let mut sensors = [None; 8];
    for (index, sensor) in sensors.iter_mut().enumerate() {
        let kelvin = u16_at(bytes, 200 + index * 2);
        *sensor = (kelvin != 0).then_some(kelvin);
    }
    Ok(HealthLog {
        critical_warning: byte_at(bytes, 0),
        temperature_kelvin: u16_at(bytes, 1),
        available_spare: byte_at(bytes, 3),
        available_spare_threshold: byte_at(bytes, 4),
        percentage_used: byte_at(bytes, 5),
        data_units_read: u128_at(bytes, 32),
        data_units_written: u128_at(bytes, 48),
        host_read_commands: u128_at(bytes, 64),
        host_write_commands: u128_at(bytes, 80),
        controller_busy_minutes: u128_at(bytes, 96),
        power_cycles: u128_at(bytes, 112),
        power_on_hours: u128_at(bytes, 128),
        unsafe_shutdowns: u128_at(bytes, 144),
        media_errors: u128_at(bytes, 160),
        error_log_entries: u128_at(bytes, 176),
        warning_temperature_minutes: u32_at(bytes, 192),
        critical_temperature_minutes: u32_at(bytes, 196),
        sensors_kelvin: sensors,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Put spans of bytes into a buffer of zeros.
    pub(crate) fn buffer(size: usize, spans: &[(usize, &str)]) -> Vec<u8> {
        let mut out = vec![0u8; size];
        for (offset, hex) in spans {
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            out[*offset..*offset + bytes.len()].copy_from_slice(&bytes);
        }
        out
    }

    /// The health log of an APPLE SSD AP0512Z, firmware 241.0.12, captured
    /// with `smartctl -r ioctl,2 -A` on 2026-10-10. Every other byte is zero.
    pub(crate) fn apple_ssd_log() -> Vec<u8> {
        buffer(
            HEALTH_LOG_BYTES,
            &[
                (0x01, "34016463"),
                (0x20, "1a551802"),
                (0x30, "bb860201"),
                (0x40, "b22dd12d"),
                (0x50, "4a6a0a0e"),
                (0x70, "c3"),
                (0x80, "6f"),
                (0x90, "0b"),
            ],
        )
    }

    /// The identify data of the same drive, captured with
    /// `smartctl -r ioctl,2 -i`. Only the first 528 bytes matter here. The
    /// serial number of the drive is replaced with `SERIALMASKED0001`, which
    /// has the same length.
    pub(crate) fn apple_ssd_identify() -> Vec<u8> {
        buffer(
            IDENTIFY_BYTES,
            &[
                (
                    0x00,
                    "6b106b1053455249414c4d41534b454430303031202020204150504c4520535344\
                     204150303531325a20202020202020202020202020202020202020202020203234\
                     312e302e3132040000000008",
                ),
                (0x100, "040000010200000001"),
                (0x148, "04"),
                (0x200, "76440000030000000400000000010000000001"),
            ],
        )
    }

    #[test]
    fn the_identify_data_of_a_real_drive_gives_what_smartctl_gave() {
        let c = controller(&apple_ssd_identify()).unwrap();
        assert_eq!(c.vendor_id, 0x106b);
        assert_eq!(c.subsystem_vendor_id, 0x106b);
        assert_eq!(c.serial.as_deref(), Some("SERIALMASKED0001"));
        assert_eq!(c.model.as_deref(), Some("APPLE SSD AP0512Z"));
        assert_eq!(c.firmware.as_deref(), Some("241.0.12"));
        assert_eq!(c.ieee_oui, 0);
        assert_eq!(c.controller_id, 0);
        // smartctl says "<1.2", which is a version of zero.
        assert_eq!(c.version, None);
        assert_eq!(c.namespaces, 3);
        assert_eq!(c.warning_temperature_kelvin, None);
        assert_eq!(c.total_capacity, None);
    }

    #[test]
    fn a_version_reads_as_major_minor_and_tertiary() {
        let mut bytes = apple_ssd_identify();
        bytes[80..84].copy_from_slice(&0x0001_0400u32.to_le_bytes());
        assert_eq!(controller(&bytes).unwrap().version, Some((1, 4, 0)));
    }

    #[test]
    fn the_temperature_thresholds_are_read_in_kelvin() {
        let mut bytes = apple_ssd_identify();
        bytes[266..268].copy_from_slice(&356u16.to_le_bytes());
        bytes[268..270].copy_from_slice(&358u16.to_le_bytes());
        let c = controller(&bytes).unwrap();
        assert_eq!(c.warning_temperature_kelvin, Some(356));
        assert_eq!(c.critical_temperature_kelvin, Some(358));
    }

    #[test]
    fn a_name_field_of_zeros_or_noise_is_absent() {
        let mut bytes = apple_ssd_identify();
        bytes[4..24].fill(0);
        bytes[24] = 0xff;
        let c = controller(&bytes).unwrap();
        assert_eq!(c.serial, None);
        assert_eq!(c.model, None);
    }

    #[test]
    fn an_identify_buffer_too_short_is_an_error_and_not_a_panic() {
        assert!(controller(&[0u8; 519]).is_err());
        assert!(controller(&[0u8; 520]).is_ok());
    }

    #[test]
    fn the_log_of_a_real_drive_gives_what_smartctl_gave() {
        // smartctl printed these values from the same bytes.
        let log = health_log(&apple_ssd_log()).unwrap();
        assert_eq!(log.critical_warning, 0);
        assert_eq!(log.temperature_celsius(), Some(35));
        assert_eq!(log.available_spare, 100);
        assert_eq!(log.available_spare_threshold, 99);
        assert_eq!(log.percentage_used, 0);
        assert_eq!(log.data_units_read, 35_149_082);
        assert_eq!(log.data_units_written, 16_942_779);
        assert_eq!(log.host_read_commands, 768_683_442);
        assert_eq!(log.host_write_commands, 235_563_594);
        assert_eq!(log.controller_busy_minutes, 0);
        assert_eq!(log.power_cycles, 195);
        assert_eq!(log.power_on_hours, 111);
        assert_eq!(log.unsafe_shutdowns, 11);
        assert_eq!(log.media_errors, 0);
        assert_eq!(log.error_log_entries, 0);
    }

    #[test]
    fn a_data_unit_is_a_thousand_sectors_of_512_bytes() {
        let log = health_log(&apple_ssd_log()).unwrap();
        // smartctl shows 17.9 TB read and 8.67 TB written.
        assert_eq!(log.bytes_read(), 35_149_082 * 512_000);
        assert_eq!(log.bytes_read() / 100_000_000_000, 179);
        assert_eq!(log.bytes_written() / 10_000_000_000, 867);
    }

    #[test]
    fn a_sensor_of_zero_kelvin_is_absent() {
        let log = health_log(&apple_ssd_log()).unwrap();
        assert_eq!(log.sensors_kelvin, [None; 8]);
        let mut bytes = apple_ssd_log();
        bytes[200..202].copy_from_slice(&318u16.to_le_bytes());
        let log = health_log(&bytes).unwrap();
        assert_eq!(log.sensors_kelvin[0], Some(318));
        assert_eq!(kelvin_to_celsius(318), Some(45));
    }

    #[test]
    fn a_composite_temperature_of_zero_is_absent() {
        let log = health_log(&vec![0u8; HEALTH_LOG_BYTES]).unwrap();
        assert_eq!(log.temperature_celsius(), None);
    }

    #[test]
    fn each_critical_warning_bit_is_read_as_the_drive_set_it() {
        let mut bytes = apple_ssd_log();
        bytes[0] = critical::SPARE | critical::READ_ONLY;
        let log = health_log(&bytes).unwrap();
        assert_ne!(log.critical_warning & critical::SPARE, 0);
        assert_ne!(log.critical_warning & critical::READ_ONLY, 0);
        assert_eq!(log.critical_warning & critical::TEMPERATURE, 0);
    }

    #[test]
    fn a_counter_uses_all_sixteen_bytes() {
        let mut bytes = apple_ssd_log();
        bytes[128..144].copy_from_slice(&u128::MAX.to_le_bytes());
        let log = health_log(&bytes).unwrap();
        assert_eq!(log.power_on_hours, u128::MAX);
        // A count of bytes that does not fit stops at the largest value.
        bytes[32..48].copy_from_slice(&u128::MAX.to_le_bytes());
        assert_eq!(health_log(&bytes).unwrap().bytes_read(), u128::MAX);
    }

    #[test]
    fn a_log_too_short_for_its_fields_is_an_error_and_not_a_panic() {
        assert!(health_log(&[]).is_err());
        assert!(health_log(&[0u8; 215]).is_err());
        assert!(health_log(&[0u8; 216]).is_ok());
    }
}
