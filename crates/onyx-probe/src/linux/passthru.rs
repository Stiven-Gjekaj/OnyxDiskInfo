//! The commands that the Linux shim sends to a drive, as plain values.
//!
//! This module makes no system call. It builds each command and reads each
//! status, so its tests run on all three hosts. The shim only copies these
//! values into the structures of the kernel.
//!
//! Each command here reads, but for the experimental erase commands of
//! [`crate::firmware`] at the end.

pub use crate::sat::{
    ATA_IDENTIFY, ATA_SMART_READ_DATA, ATA_SMART_READ_THRESHOLDS, AtaCommand, ata_pass_through_16,
};

/// One NVMe admin command, as `struct nvme_passthru_cmd` holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub nsid: u32,
    pub cdw10: u32,
    pub data_len: u32,
    /// Zero lets the kernel use its own timeout, which is 60 seconds.
    pub timeout_ms: u32,
}

/// Identify Controller: opcode 0x06, CNS 1 in command dword 10.
pub const NVME_IDENTIFY_CONTROLLER: NvmeCommand = NvmeCommand {
    opcode: 0x06,
    nsid: 0,
    cdw10: 1,
    data_len: 4096,
    timeout_ms: 0,
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
    timeout_ms: 0,
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
    out[64..68].copy_from_slice(&command.timeout_ms.to_ne_bytes());
    out
}

/// `NVME_IOCTL_ID`: `_IO('N', 0x40)`. It returns the namespace identifier of
/// the node.
pub const NVME_IOCTL_ID: u64 = ioc(0, b'N', 0x40, 0);

/// Identify Namespace: opcode 0x06, CNS 0, for one namespace.
pub fn nvme_identify_namespace(nsid: u32) -> NvmeCommand {
    NvmeCommand {
        opcode: 0x06,
        nsid,
        cdw10: 0,
        data_len: 4096,
        timeout_ms: 0,
    }
}

/// The time that a format gets, in milliseconds: three hours. A format with
/// User Data Erase can write every block.
pub const NVME_FORMAT_TIMEOUT_MS: u32 = 3 * 60 * 60 * 1000;

/// Format NVM, opcode 0x80, with command dword 10 from
/// [`crate::firmware::format_cdw10`]. Experimental.
pub fn nvme_format(nsid: u32, cdw10: u32) -> NvmeCommand {
    NvmeCommand {
        opcode: 0x80,
        nsid,
        cdw10,
        data_len: 0,
        timeout_ms: NVME_FORMAT_TIMEOUT_MS,
    }
}

/// Sanitize, opcode 0x84, for the whole controller, with command dword 10
/// from [`crate::firmware::sanitize_cdw10`]. Experimental. The drive starts
/// the sanitize and answers at once.
pub fn nvme_sanitize(cdw10: u32) -> NvmeCommand {
    NvmeCommand {
        opcode: 0x84,
        nsid: 0,
        cdw10,
        data_len: 0,
        timeout_ms: 0,
    }
}

/// Get Log Page for the Sanitize Status log, log 0x81, of 512 bytes.
pub const NVME_SANITIZE_LOG: NvmeCommand = NvmeCommand {
    opcode: 0x02,
    nsid: 0xffff_ffff,
    cdw10: (128 - 1) << 16 | 0x81,
    data_len: 512,
    timeout_ms: 0,
};

/// `SG_IO`, the ioctl that sends a SCSI command.
pub const SG_IO: u64 = 0x2285;

/// `SG_DXFER_FROM_DEV`.
pub const SG_DXFER_FROM_DEV: i32 = -3;

/// `SG_DXFER_TO_DEV`.
pub const SG_DXFER_TO_DEV: i32 = -2;

/// `SG_DXFER_NONE`.
pub const SG_DXFER_NONE: i32 = -1;

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
    fn a_format_carries_its_timeout_after_the_six_command_dwords() {
        let bytes = nvme_command_bytes(nvme_format(1, 0x200), 0);
        assert_eq!(bytes[0], 0x80);
        assert_eq!(&bytes[4..8], &1u32.to_ne_bytes());
        assert_eq!(&bytes[40..44], &0x200u32.to_ne_bytes());
        assert_eq!(&bytes[64..68], &NVME_FORMAT_TIMEOUT_MS.to_ne_bytes());
        assert_eq!(&bytes[36..40], &0u32.to_ne_bytes());
    }

    #[test]
    fn the_erase_commands_have_the_numbers_of_the_standard() {
        // From <linux/nvme_ioctl.h>.
        assert_eq!(NVME_IOCTL_ID, 0x4e40);
        assert_eq!(nvme_sanitize(4).opcode, 0x84);
        assert_eq!(nvme_sanitize(4).nsid, 0);
        assert_eq!(NVME_SANITIZE_LOG.cdw10, 0x007f_0081);
        let ns = nvme_identify_namespace(3);
        assert_eq!(
            (ns.opcode, ns.nsid, ns.cdw10, ns.data_len),
            (0x06, 3, 0, 4096)
        );
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
