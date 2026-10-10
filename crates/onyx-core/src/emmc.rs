//! What an eMMC chip reports about its wear.
//!
//! An eMMC chip has no SMART. Version 5.0 of the JEDEC standard (JESD84-B50)
//! adds three bytes to EXT_CSD that estimate its wear, and Linux shows them
//! as two text files: `life_time` and `pre_eol_info`.

use crate::{Error, Result};

/// The wear estimates of an eMMC chip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Life {
    /// `DEVICE_LIFE_TIME_EST_TYP_A`: the wear of the SLC area, in steps of
    /// ten percent. 1 is 0 to 10 percent, 10 is 90 to 100 percent, and 11
    /// is past the rated life. `None` means the chip does not say.
    pub type_a: Option<u8>,
    /// `DEVICE_LIFE_TIME_EST_TYP_B`: the same for the MLC area.
    pub type_b: Option<u8>,
    /// `PRE_EOL_INFO`: 1 is normal, 2 is a warning, and 3 is urgent. The
    /// warning comes when the chip has used 80 percent of its reserve
    /// blocks.
    pub pre_eol: Option<u8>,
}

/// The step of a life estimate that means past the rated life.
pub const PAST_RATED_LIFE: u8 = 0x0b;

impl Life {
    /// The higher of the two wear estimates, as a range of percent used.
    pub fn used_percent(&self) -> Option<(u8, u8)> {
        let step = self.type_a.max(self.type_b)?;
        step_percent(step)
    }
}

/// The range of percent used that a step names. Step 11 is past the rated
/// life, and its range starts at 100.
pub fn step_percent(step: u8) -> Option<(u8, u8)> {
    match step {
        1..=10 => Some(((step - 1) * 10, step * 10)),
        PAST_RATED_LIFE => Some((100, 100)),
        _ => None,
    }
}

/// Read one value as the kernel writes it: `0x01`.
fn hex_byte(text: &str) -> Option<u8> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").unwrap_or(text);
    u8::from_str_radix(digits, 16).ok().filter(|v| *v != 0)
}

/// Read the two files of `sysfs`: `life_time` holds the two estimates, as in
/// `0x01 0x02`, and `pre_eol_info` holds one value.
///
/// A value of zero means the chip does not say, and it reads as absent. A
/// chip that gives neither file is an error, because it has no data at all.
pub fn from_sysfs(life_time: Option<&str>, pre_eol_info: Option<&str>) -> Result<Life> {
    if life_time.is_none() && pre_eol_info.is_none() {
        return Err(Error::host(
            "eMMC",
            "the chip gives no life_time and no pre_eol_info",
        ));
    }
    let mut estimates = life_time.unwrap_or("").split_whitespace().map(hex_byte);
    Ok(Life {
        type_a: estimates.next().flatten(),
        type_b: estimates.next().flatten(),
        pre_eol: pre_eol_info.and_then(hex_byte),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_text_gives_both_estimates_and_the_reserve() {
        let life = from_sysfs(Some("0x01 0x02\n"), Some("0x01\n")).unwrap();
        assert_eq!(life.type_a, Some(1));
        assert_eq!(life.type_b, Some(2));
        assert_eq!(life.pre_eol, Some(1));
    }

    #[test]
    fn the_wear_is_the_higher_estimate_as_a_range() {
        let life = from_sysfs(Some("0x01 0x03"), None).unwrap();
        assert_eq!(life.used_percent(), Some((20, 30)));
    }

    #[test]
    fn each_step_names_ten_percent_and_step_eleven_is_past_the_end() {
        assert_eq!(step_percent(1), Some((0, 10)));
        assert_eq!(step_percent(10), Some((90, 100)));
        assert_eq!(step_percent(11), Some((100, 100)));
        assert_eq!(step_percent(0), None);
        assert_eq!(step_percent(12), None);
    }

    #[test]
    fn a_value_of_zero_is_absent() {
        let life = from_sysfs(Some("0x00 0x00"), Some("0x00")).unwrap();
        assert_eq!(life, Life::default());
        assert_eq!(life.used_percent(), None);
    }

    #[test]
    fn a_chip_with_only_one_file_still_gives_what_it_has() {
        let life = from_sysfs(None, Some("0x02")).unwrap();
        assert_eq!(life.pre_eol, Some(2));
        assert_eq!(life.type_a, None);
    }

    #[test]
    fn a_chip_with_neither_file_is_an_error() {
        assert!(from_sysfs(None, None).is_err());
    }

    #[test]
    fn text_that_is_not_a_number_is_absent_and_not_a_panic() {
        let life = from_sysfs(Some("zz"), Some("")).unwrap();
        assert_eq!(life, Life::default());
    }
}
