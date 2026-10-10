//! The commands that the Linux shim sends to a drive, as plain values.
//!
//! This module makes no system call. It builds each command and reads each
//! status, so its tests run on all three hosts. The shim only copies these
//! values into the structures of the kernel.
//!
//! Each command here reads. None of them changes the drive.

/// One NVMe admin command, as `struct nvme_passthru_cmd` holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub nsid: u32,
    pub cdw10: u32,
    pub data_len: u32,
}

/// Identify Controller: opcode 0x06, CNS 1 in command dword 10.
pub const NVME_IDENTIFY_CONTROLLER: NvmeCommand = NvmeCommand {
    opcode: 0x06,
    nsid: 0,
    cdw10: 1,
    data_len: 4096,
};

/// Get Log Page for the SMART / Health Information log, for the whole
/// controller.
///
/// Command dword 10 holds the log identifier in its low byte and the number
/// of dwords less one in its high half. 512 bytes are 128 dwords.
pub const NVME_HEALTH_LOG: NvmeCommand = NvmeCommand {
    opcode: 0x02,
    nsid: 0xffff_ffff,
    cdw10: (128 - 1) << 16 | 0x02,
    data_len: 512,
};

/// `NVME_IOCTL_ADMIN_CMD`: `_IOWR('N', 0x41, struct nvme_admin_cmd)`, and
/// the structure is 72 bytes.
pub const NVME_IOCTL_ADMIN_CMD: u64 = ioc(3, b'N', 0x41, 72);

/// The size of `struct nvme_passthru_cmd`, in bytes.
pub const NVME_COMMAND_BYTES: usize = 72;

/// Build an ioctl number the way the kernel macro `_IOC` does.
const fn ioc(direction: u64, kind: u8, number: u8, size: u64) -> u64 {
    direction << 30 | size << 16 | (kind as u64) << 8 | number as u64
}

/// The bytes of `struct nvme_passthru_cmd` for one command, with the
/// address of the buffer that the drive fills.
///
/// The layout: opcode, flags, two reserved bytes, nsid, cdw2, cdw3, the
/// metadata address, the data address, the metadata length, the data
/// length, cdw10 to cdw15, the timeout, and the result.
pub fn nvme_command_bytes(command: NvmeCommand, data_address: u64) -> [u8; NVME_COMMAND_BYTES] {
    let mut out = [0u8; NVME_COMMAND_BYTES];
    out[0] = command.opcode;
    out[4..8].copy_from_slice(&command.nsid.to_ne_bytes());
    out[24..32].copy_from_slice(&data_address.to_ne_bytes());
    out[36..40].copy_from_slice(&command.data_len.to_ne_bytes());
    out[40..44].copy_from_slice(&command.cdw10.to_ne_bytes());
    out
}

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

/// `SG_IO`, the ioctl that sends a SCSI command.
pub const SG_IO: u64 = 0x2285;

/// `SG_DXFER_FROM_DEV`.
pub const SG_DXFER_FROM_DEV: i32 = -3;

/// The status fields that `SG_IO` writes back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SgStatus {
    pub status: u8,
    pub host_status: u16,
    pub driver_status: u16,
}

/// The `DRIVER_SENSE` bit of `driver_status`: the device gave sense data.
const DRIVER_SENSE: u16 = 0x08;

impl SgStatus {
    /// The command reached the drive and the drive did it.
    ///
    /// A status of CHECK CONDITION means the drive or the bridge refused the
    /// command. A virtual SCSI disk refuses ATA PASS-THROUGH this way, and
    /// so does a USB bridge that does not speak SAT.
    pub fn succeeded(self) -> bool {
        self.status == 0 && self.host_status == 0 && self.driver_status & !DRIVER_SENSE == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_admin_ioctl_has_the_number_that_the_kernel_gives_it() {
        // From <linux/nvme_ioctl.h>, as the kernel headers compute it.
        assert_eq!(NVME_IOCTL_ADMIN_CMD, 0xc048_4e41);
    }

    #[test]
    fn the_health_log_asks_for_128_dwords_of_log_two() {
        assert_eq!(NVME_HEALTH_LOG.cdw10, 0x007f_0002);
        assert_eq!(NVME_HEALTH_LOG.nsid, 0xffff_ffff);
        assert_eq!(NVME_HEALTH_LOG.data_len as usize, 512);
    }

    #[test]
    fn identify_asks_for_the_controller_with_cns_one() {
        assert_eq!(NVME_IDENTIFY_CONTROLLER.opcode, 0x06);
        assert_eq!(NVME_IDENTIFY_CONTROLLER.cdw10, 1);
        assert_eq!(NVME_IDENTIFY_CONTROLLER.data_len, 4096);
    }

    #[test]
    fn each_field_of_the_nvme_command_is_at_its_offset() {
        let bytes = nvme_command_bytes(NVME_HEALTH_LOG, 0x1122_3344_5566_7788);
        assert_eq!(bytes[0], 0x02);
        assert_eq!(&bytes[4..8], &0xffff_ffffu32.to_ne_bytes());
        assert_eq!(&bytes[24..32], &0x1122_3344_5566_7788u64.to_ne_bytes());
        assert_eq!(&bytes[36..40], &512u32.to_ne_bytes());
        assert_eq!(&bytes[40..44], &0x007f_0002u32.to_ne_bytes());
        // The metadata address and length, the flags and the timeout stay
        // zero, and the kernel then uses its own timeout.
        assert!(bytes[8..24].iter().all(|b| *b == 0));
        assert!(bytes[32..36].iter().all(|b| *b == 0));
        assert!(bytes[44..].iter().all(|b| *b == 0));
    }

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
    fn a_command_succeeds_only_when_every_status_is_clear() {
        assert!(SgStatus::default().succeeded());
        let check_condition = SgStatus {
            status: 0x02,
            host_status: 0,
            driver_status: DRIVER_SENSE,
        };
        assert!(!check_condition.succeeded());
        let host_error = SgStatus {
            status: 0,
            host_status: 0x07,
            driver_status: 0,
        };
        assert!(!host_error.succeeded());
    }
}
