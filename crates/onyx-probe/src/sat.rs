//! The ATA commands that Onyx sends, and the SCSI command that carries them.
//!
//! SAT, the SCSI / ATA Translation standard, defines ATA PASS-THROUGH. A SCSI
//! layer passes it to a SATA drive, and so does a USB bridge that speaks SAT.
//! Linux and Windows send the same bytes, so they live here once.
//!
//! This module makes no system call. Each command of [`AtaCommand`] reads.
//! The two other builders carry the experimental erase commands of
//! [`crate::firmware`].

/// One ATA command that reads 512 bytes from the drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtaCommand {
    pub command: u8,
    pub features: u8,
    pub lba_mid: u8,
    pub lba_high: u8,
}

/// IDENTIFY DEVICE.
pub const ATA_IDENTIFY: AtaCommand = AtaCommand {
    command: 0xec,
    features: 0,
    lba_mid: 0,
    lba_high: 0,
};

/// SMART READ DATA. Every SMART command carries 0x4F and 0xC2 in the two
/// address bytes, as a key.
pub const ATA_SMART_READ_DATA: AtaCommand = AtaCommand {
    command: 0xb0,
    features: 0xd0,
    lba_mid: 0x4f,
    lba_high: 0xc2,
};

/// SMART READ THRESHOLDS.
pub const ATA_SMART_READ_THRESHOLDS: AtaCommand = AtaCommand {
    command: 0xb0,
    features: 0xd1,
    lba_mid: 0x4f,
    lba_high: 0xc2,
};

/// The SCSI command ATA PASS-THROUGH (16), from the SAT standard, for one
/// ATA command that reads one block of 512 bytes with PIO.
pub fn ata_pass_through_16(command: AtaCommand) -> [u8; 16] {
    // Protocol 4 is PIO data-in, in bits 1 to 4.
    const PIO_DATA_IN: u8 = 4 << 1;
    // T_DIR 1: from the device. BYT_BLOK 1: the count is in blocks.
    // T_LENGTH 2: the count is in the sector count field.
    const FROM_DEVICE_IN_BLOCKS: u8 = 1 << 3 | 1 << 2 | 2;
    let mut cdb = [0u8; 16];
    cdb[0] = 0x85;
    cdb[1] = PIO_DATA_IN;
    cdb[2] = FROM_DEVICE_IN_BLOCKS;
    cdb[4] = command.features;
    cdb[6] = 1; // one block
    cdb[10] = command.lba_mid;
    cdb[12] = command.lba_high;
    cdb[14] = command.command;
    cdb
}

/// ATA PASS-THROUGH (16) for one ATA command that writes one block of 512
/// bytes to the drive with PIO.
pub fn ata_pass_through_16_out(command: u8) -> [u8; 16] {
    // Protocol 5 is PIO data-out, in bits 1 to 4.
    const PIO_DATA_OUT: u8 = 5 << 1;
    // T_DIR 0: to the device. BYT_BLOK 1 and T_LENGTH 2, as above.
    const TO_DEVICE_IN_BLOCKS: u8 = 1 << 2 | 2;
    let mut cdb = [0u8; 16];
    cdb[0] = 0x85;
    cdb[1] = PIO_DATA_OUT;
    cdb[2] = TO_DEVICE_IN_BLOCKS;
    cdb[6] = 1; // one block
    cdb[14] = command;
    cdb
}

/// ATA PASS-THROUGH (16) for one ATA command that moves no data.
pub fn ata_pass_through_16_none(command: u8) -> [u8; 16] {
    // Protocol 3 is non-data, in bits 1 to 4. T_LENGTH 0: no transfer.
    const NON_DATA: u8 = 3 << 1;
    let mut cdb = [0u8; 16];
    cdb[0] = 0x85;
    cdb[1] = NON_DATA;
    cdb[14] = command;
    cdb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pass_through_reads_one_block_with_pio() {
        let cdb = ata_pass_through_16(ATA_IDENTIFY);
        assert_eq!(cdb[0], 0x85);
        assert_eq!(cdb[1], 0x08);
        assert_eq!(cdb[2], 0x0e);
        assert_eq!(cdb[6], 1);
        assert_eq!(cdb[14], 0xec);
    }

    #[test]
    fn a_smart_command_carries_its_key_and_its_feature() {
        let cdb = ata_pass_through_16(ATA_SMART_READ_DATA);
        assert_eq!(
            (cdb[4], cdb[10], cdb[12], cdb[14]),
            (0xd0, 0x4f, 0xc2, 0xb0)
        );
        let cdb = ata_pass_through_16(ATA_SMART_READ_THRESHOLDS);
        assert_eq!(cdb[4], 0xd1);
    }

    #[test]
    fn a_command_that_writes_sends_one_block_to_the_drive() {
        let cdb = ata_pass_through_16_out(0xf4);
        assert_eq!(
            (cdb[0], cdb[1], cdb[2], cdb[6], cdb[14]),
            (0x85, 0x0a, 0x06, 1, 0xf4)
        );
    }

    #[test]
    fn a_command_with_no_data_moves_no_block() {
        let cdb = ata_pass_through_16_none(0xf3);
        assert_eq!(
            (cdb[0], cdb[1], cdb[2], cdb[6], cdb[14]),
            (0x85, 0x06, 0, 0, 0xf3)
        );
    }
}
