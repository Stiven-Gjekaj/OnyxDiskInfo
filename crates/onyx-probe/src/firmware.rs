//! The erase commands of the firmware, as plain values. Experimental: no
//! drive has taken one from Onyx yet.
//!
//! This module makes no system call. It reads the identify data of a drive,
//! decides whether the drive can take an erase, and builds each command. The
//! shims send the commands. The tests run on all three hosts.
//!
//! ATA Secure Erase sets a user password, then sends SECURITY ERASE PREPARE
//! and SECURITY ERASE UNIT. A drive that finishes the erase clears the
//! password itself. A drive that stops in the middle keeps it, so the
//! password is a constant that each message names.

/// The erase commands that Onyx can send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// ATA SECURITY ERASE UNIT, normal mode: the drive writes zeros or ones
    /// over the user data.
    AtaSecureErase,
    /// ATA SECURITY ERASE UNIT, enhanced mode: the drive writes a pattern
    /// of the vendor over every block, the spare blocks too.
    AtaEnhancedSecureErase,
    /// NVMe Format NVM with User Data Erase.
    NvmeFormat,
    /// NVMe Format NVM with Cryptographic Erase.
    NvmeCryptoFormat,
    /// NVMe Sanitize with Block Erase.
    NvmeBlockSanitize,
    /// NVMe Sanitize with Crypto Erase.
    NvmeCryptoSanitize,
}

impl Method {
    /// The name of the method, as a person reads it.
    pub fn name(self) -> &'static str {
        match self {
            Method::AtaSecureErase => "ATA Secure Erase",
            Method::AtaEnhancedSecureErase => "ATA Enhanced Secure Erase",
            Method::NvmeFormat => "NVMe Format with User Data Erase",
            Method::NvmeCryptoFormat => "NVMe Format with Cryptographic Erase",
            Method::NvmeBlockSanitize => "NVMe Sanitize with Block Erase",
            Method::NvmeCryptoSanitize => "NVMe Sanitize with Crypto Erase",
        }
    }

    /// Whether the method is for an ATA drive, and not for an NVMe drive.
    pub fn is_ata(self) -> bool {
        matches!(
            self,
            Method::AtaSecureErase | Method::AtaEnhancedSecureErase
        )
    }
}

/// The user password that Onyx sets before an ATA erase.
pub const ATA_PASSWORD: &str = "onyx";

/// SECURITY SET PASSWORD.
pub const ATA_SECURITY_SET_PASSWORD: u8 = 0xf1;

/// SECURITY ERASE PREPARE.
pub const ATA_SECURITY_ERASE_PREPARE: u8 = 0xf3;

/// SECURITY ERASE UNIT.
pub const ATA_SECURITY_ERASE_UNIT: u8 = 0xf4;

/// What word 128 and words 89 and 90 of IDENTIFY DEVICE say about the ATA
/// security feature set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtaSecurity {
    pub supported: bool,
    pub enabled: bool,
    pub locked: bool,
    pub frozen: bool,
    pub count_expired: bool,
    pub enhanced_supported: bool,
    /// The time that the drive gives for a normal erase, in minutes.
    pub normal_minutes: Option<u32>,
    /// The time that the drive gives for an enhanced erase, in minutes.
    pub enhanced_minutes: Option<u32>,
}

fn word(identify: &[u8], index: usize) -> u16 {
    identify
        .get(index * 2..index * 2 + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

/// The time of an erase from word 89 or word 90, in minutes.
///
/// Bit 15 set: bits 0 to 14 give the time in units of two minutes. Bit 15
/// clear: bits 0 to 7 do. Zero means the drive gives no time.
fn erase_minutes(value: u16) -> Option<u32> {
    let units = if value & 0x8000 != 0 {
        value & 0x7fff
    } else {
        value & 0x00ff
    };
    (units != 0).then_some(u32::from(units) * 2)
}

/// Read the security state from 512 bytes of IDENTIFY DEVICE.
pub fn ata_security(identify: &[u8]) -> AtaSecurity {
    let status = word(identify, 128);
    AtaSecurity {
        supported: status & 1 != 0,
        enabled: status & 1 << 1 != 0,
        locked: status & 1 << 2 != 0,
        frozen: status & 1 << 3 != 0,
        count_expired: status & 1 << 4 != 0,
        enhanced_supported: status & 1 << 5 != 0,
        normal_minutes: erase_minutes(word(identify, 89)),
        enhanced_minutes: erase_minutes(word(identify, 90)),
    }
}

/// Why the drive cannot take the ATA erase, or `None` when it can.
pub fn ata_refusal(security: &AtaSecurity, method: Method) -> Option<String> {
    if !security.supported {
        return Some("The drive has no ATA security feature set.".to_string());
    }
    if security.enabled || security.locked {
        return Some(
            "The drive has a password already. Onyx does not erase a drive with a password \
             that Onyx did not set."
                .to_string(),
        );
    }
    if security.count_expired {
        return Some(
            "The drive took too many wrong passwords. Turn it off and on, and try again."
                .to_string(),
        );
    }
    if security.frozen {
        return Some(
            "The drive is frozen. The firmware of the computer froze it at start, and a frozen \
             drive refuses each security command. Put the computer to sleep and wake it, or \
             plug the drive in again, and try again."
                .to_string(),
        );
    }
    if method == Method::AtaEnhancedSecureErase && !security.enhanced_supported {
        return Some("The drive has no enhanced secure erase.".to_string());
    }
    None
}

/// The time that the shim gives SECURITY ERASE UNIT, in milliseconds: two
/// times the time of the drive and 30 minutes more, or 12 hours when the
/// drive gives no time.
pub fn ata_erase_timeout_ms(security: &AtaSecurity, method: Method) -> u32 {
    let minutes = if method == Method::AtaEnhancedSecureErase {
        security.enhanced_minutes
    } else {
        security.normal_minutes
    };
    let minutes = minutes.map_or(12 * 60, |m| m * 2 + 30);
    minutes.saturating_mul(60_000)
}

/// The 512 bytes that SECURITY SET PASSWORD and SECURITY ERASE UNIT send.
///
/// Word 0 holds the control bits. Bit 0 clear: the user password. Bit 1 set,
/// for SECURITY ERASE UNIT only: the enhanced mode. Bit 8 clear, for SECURITY
/// SET PASSWORD only: the high level of security. Words 1 to 16 hold the
/// password, padded with zeros.
pub fn ata_password_block(enhanced: bool) -> [u8; 512] {
    let mut block = [0u8; 512];
    if enhanced {
        block[0] = 1 << 1;
    }
    let password = ATA_PASSWORD.as_bytes();
    block[2..2 + password.len()].copy_from_slice(password);
    block
}

/// What a person types to take the password off a drive that stopped in the
/// middle of an erase.
pub fn ata_unlock_hint(node: &str) -> String {
    format!(
        "The drive may keep the password \"{ATA_PASSWORD}\". To take it off: \
         hdparm --user-master u --security-disable {ATA_PASSWORD} {node}"
    )
}

/// What Identify Controller says about the erases of an NVMe drive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NvmeErase {
    /// SANICAP bit 0: Sanitize with Crypto Erase.
    pub sanitize_crypto: bool,
    /// SANICAP bit 1: Sanitize with Block Erase.
    pub sanitize_block: bool,
    /// FNA bit 2: Format with Cryptographic Erase.
    pub format_crypto: bool,
    /// NN: the highest namespace identifier.
    pub namespaces: u32,
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    bytes
        .get(offset..offset + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read the erase fields from 4096 bytes of Identify Controller.
pub fn nvme_erase(identify: &[u8]) -> NvmeErase {
    let sanicap = dword(identify, 328);
    let fna = identify.get(524).copied().unwrap_or(0);
    NvmeErase {
        sanitize_crypto: sanicap & 1 != 0,
        sanitize_block: sanicap & 1 << 1 != 0,
        format_crypto: fna & 1 << 2 != 0,
        namespaces: dword(identify, 516),
    }
}

/// Why the drive cannot take the NVMe erase, or `None` when it can.
pub fn nvme_refusal(erase: &NvmeErase, method: Method) -> Option<String> {
    if erase.namespaces != 1 {
        return Some(format!(
            "The drive can hold {} namespaces. An erase of the firmware can reach each one, \
             and Onyx erases only a drive with one.",
            erase.namespaces
        ));
    }
    let (supported, what) = match method {
        Method::NvmeFormat => (true, ""),
        Method::NvmeCryptoFormat => (erase.format_crypto, "Format with Cryptographic Erase"),
        Method::NvmeBlockSanitize => (erase.sanitize_block, "Sanitize with Block Erase"),
        Method::NvmeCryptoSanitize => (erase.sanitize_crypto, "Sanitize with Crypto Erase"),
        Method::AtaSecureErase | Method::AtaEnhancedSecureErase => {
            return Some("An ATA erase does not go to an NVMe drive.".to_string());
        }
    };
    (!supported).then(|| format!("The drive has no {what}."))
}

/// Command dword 10 of Format NVM.
///
/// The format keeps the block size and the protection of the namespace, from
/// FLBAS (byte 26) and DPS (byte 29) of Identify Namespace. Only the Secure
/// Erase Settings change: 1 is User Data Erase, 2 is Cryptographic Erase.
pub fn format_cdw10(identify_namespace: &[u8], crypto: bool) -> u32 {
    let flbas = u32::from(identify_namespace.get(26).copied().unwrap_or(0));
    let dps = u32::from(identify_namespace.get(29).copied().unwrap_or(0));
    let lbaf_low = flbas & 0x0f;
    let lbaf_high = flbas >> 5 & 0x03;
    let metadata_settings = flbas >> 4 & 1;
    let protection = dps & 0x07;
    let protection_location = dps >> 3 & 1;
    let erase = if crypto { 2 } else { 1 };
    lbaf_low
        | metadata_settings << 4
        | protection << 5
        | protection_location << 8
        | erase << 9
        | lbaf_high << 12
}

/// Command dword 10 of Sanitize: action 2 is Block Erase, action 4 is
/// Crypto Erase. Each other bit stays clear.
pub fn sanitize_cdw10(crypto: bool) -> u32 {
    if crypto { 4 } else { 2 }
}

/// What the Sanitize Status log says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SanitizeState {
    /// The drive was never sanitized.
    Never,
    /// The last sanitize finished.
    Done,
    /// A sanitize runs, and this part of it is done, from 0 to 1.
    Running(f64),
    /// The last sanitize failed.
    Failed,
    /// A state that the standard of today does not name.
    Other(u8),
}

/// Read the first four bytes of the Sanitize Status log: SPROG and SSTAT.
pub fn sanitize_state(log: &[u8]) -> SanitizeState {
    let progress = u16::from_le_bytes([
        log.first().copied().unwrap_or(0),
        log.get(1).copied().unwrap_or(0),
    ]);
    let status = log.get(2).copied().unwrap_or(0) & 0x07;
    match status {
        0 => SanitizeState::Never,
        // 4 is a finish in which the drive did not deallocate the blocks.
        1 | 4 => SanitizeState::Done,
        2 => SanitizeState::Running(f64::from(progress) / 65536.0),
        3 => SanitizeState::Failed,
        other => SanitizeState::Other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identify_with(word128: u16, word89: u16, word90: u16) -> Vec<u8> {
        let mut id = vec![0u8; 512];
        id[256..258].copy_from_slice(&word128.to_le_bytes());
        id[178..180].copy_from_slice(&word89.to_le_bytes());
        id[180..182].copy_from_slice(&word90.to_le_bytes());
        id
    }

    #[test]
    fn word_128_gives_each_bit_of_the_security_state() {
        let s = ata_security(&identify_with(0b10_1001, 0, 0));
        assert!(s.supported && s.frozen && s.enhanced_supported);
        assert!(!s.enabled && !s.locked && !s.count_expired);
        let s = ata_security(&identify_with(0b1_0110, 0, 0));
        assert!(s.enabled && s.locked && s.count_expired && !s.supported);
    }

    #[test]
    fn the_erase_time_reads_both_forms_of_words_89_and_90() {
        // The short form: bits 0 to 7, in units of two minutes.
        let s = ata_security(&identify_with(1, 0x0010, 0));
        assert_eq!(s.normal_minutes, Some(32));
        assert_eq!(s.enhanced_minutes, None);
        // The long form: bit 15 set, bits 0 to 14.
        let s = ata_security(&identify_with(1, 0, 0x8000 | 300));
        assert_eq!(s.enhanced_minutes, Some(600));
    }

    #[test]
    fn a_drive_that_is_not_ready_is_refused_with_its_reason() {
        let ready = AtaSecurity {
            supported: true,
            enhanced_supported: true,
            ..AtaSecurity::default()
        };
        assert_eq!(ata_refusal(&ready, Method::AtaEnhancedSecureErase), None);
        let cases = [
            (AtaSecurity::default(), "no ATA security"),
            (
                AtaSecurity {
                    enabled: true,
                    ..ready
                },
                "password already",
            ),
            (
                AtaSecurity {
                    locked: true,
                    ..ready
                },
                "password already",
            ),
            (
                AtaSecurity {
                    count_expired: true,
                    ..ready
                },
                "too many",
            ),
            (
                AtaSecurity {
                    frozen: true,
                    ..ready
                },
                "frozen",
            ),
        ];
        for (state, words) in cases {
            let why = ata_refusal(&state, Method::AtaSecureErase).unwrap();
            assert!(why.contains(words), "{why}");
        }
        let no_enhanced = AtaSecurity {
            enhanced_supported: false,
            ..ready
        };
        assert_eq!(ata_refusal(&no_enhanced, Method::AtaSecureErase), None);
        assert!(
            ata_refusal(&no_enhanced, Method::AtaEnhancedSecureErase)
                .unwrap()
                .contains("no enhanced")
        );
    }

    #[test]
    fn the_timeout_is_twice_the_time_of_the_drive_and_half_an_hour() {
        let s = AtaSecurity {
            normal_minutes: Some(10),
            enhanced_minutes: Some(60),
            ..AtaSecurity::default()
        };
        assert_eq!(
            ata_erase_timeout_ms(&s, Method::AtaSecureErase),
            50 * 60_000
        );
        assert_eq!(
            ata_erase_timeout_ms(&s, Method::AtaEnhancedSecureErase),
            150 * 60_000
        );
        let unknown = AtaSecurity::default();
        assert_eq!(
            ata_erase_timeout_ms(&unknown, Method::AtaSecureErase),
            12 * 60 * 60_000
        );
    }

    #[test]
    fn the_password_block_holds_the_mode_and_the_password() {
        let normal = ata_password_block(false);
        assert_eq!(&normal[0..2], &[0, 0]);
        assert_eq!(&normal[2..6], b"onyx");
        assert!(normal[6..].iter().all(|b| *b == 0));
        let enhanced = ata_password_block(true);
        assert_eq!(enhanced[0], 0b10);
        assert_eq!(&enhanced[2..6], b"onyx");
    }

    #[test]
    fn the_unlock_hint_names_the_password_and_the_node() {
        let hint = ata_unlock_hint("/dev/sdb");
        assert!(hint.contains("--security-disable onyx /dev/sdb"));
    }

    fn controller(sanicap: u32, fna: u8, namespaces: u32) -> Vec<u8> {
        let mut id = vec![0u8; 4096];
        id[328..332].copy_from_slice(&sanicap.to_le_bytes());
        id[516..520].copy_from_slice(&namespaces.to_le_bytes());
        id[524] = fna;
        id
    }

    #[test]
    fn identify_controller_gives_the_erases_of_the_drive() {
        let e = nvme_erase(&controller(0b11, 0b100, 1));
        assert!(e.sanitize_crypto && e.sanitize_block && e.format_crypto);
        assert_eq!(e.namespaces, 1);
        let e = nvme_erase(&controller(0b10, 0, 1));
        assert!(!e.sanitize_crypto && e.sanitize_block && !e.format_crypto);
    }

    #[test]
    fn an_nvme_drive_is_refused_an_erase_that_it_does_not_have() {
        let plain = nvme_erase(&controller(0, 0, 1));
        assert_eq!(nvme_refusal(&plain, Method::NvmeFormat), None);
        for method in [
            Method::NvmeCryptoFormat,
            Method::NvmeBlockSanitize,
            Method::NvmeCryptoSanitize,
            Method::AtaSecureErase,
        ] {
            assert!(nvme_refusal(&plain, method).is_some(), "{method:?}");
        }
        let full = nvme_erase(&controller(0b11, 0b100, 1));
        assert_eq!(nvme_refusal(&full, Method::NvmeCryptoSanitize), None);
        let shared = nvme_erase(&controller(0b11, 0b100, 4));
        assert!(
            nvme_refusal(&shared, Method::NvmeFormat)
                .unwrap()
                .contains("4 namespaces")
        );
    }

    #[test]
    fn the_format_keeps_the_layout_of_the_namespace() {
        let mut ns = vec![0u8; 4096];
        // LBA format 1, metadata in a buffer of its own, and the two high
        // bits of a format index above 15.
        ns[26] = 0b0110_0001;
        // Protection type 2, in the first bytes of the metadata.
        ns[29] = 0b1010;
        let cdw10 = format_cdw10(&ns, false);
        assert_eq!(cdw10 & 0x0f, 1);
        assert_eq!(cdw10 >> 4 & 1, 0);
        assert_eq!(cdw10 >> 5 & 0x07, 2);
        assert_eq!(cdw10 >> 8 & 1, 1);
        assert_eq!(cdw10 >> 9 & 0x07, 1);
        assert_eq!(cdw10 >> 12 & 0x03, 3);
        assert_eq!(format_cdw10(&ns, true) >> 9 & 0x07, 2);
        // A plain namespace: format 0, no protection, user data erase.
        assert_eq!(format_cdw10(&[0u8; 4096], false), 1 << 9);
    }

    #[test]
    fn sanitize_asks_for_block_erase_or_crypto_erase() {
        assert_eq!(sanitize_cdw10(false), 2);
        assert_eq!(sanitize_cdw10(true), 4);
    }

    #[test]
    fn the_sanitize_log_gives_the_state_and_the_progress() {
        assert_eq!(sanitize_state(&[0, 0, 0, 0]), SanitizeState::Never);
        assert_eq!(sanitize_state(&[0, 0, 1, 0]), SanitizeState::Done);
        assert_eq!(sanitize_state(&[0, 0, 4, 0]), SanitizeState::Done);
        assert_eq!(sanitize_state(&[0, 0, 3, 0]), SanitizeState::Failed);
        assert_eq!(
            sanitize_state(&[0x00, 0x80, 2, 0]),
            SanitizeState::Running(0.5)
        );
        assert_eq!(sanitize_state(&[0, 0, 7, 0]), SanitizeState::Other(7));
    }
}
