//! The buffers that the Windows shim sends for health data, as plain bytes.
//!
//! This module makes no system call and names no Windows type. It builds
//! each buffer and reads each answer, so its tests run on all three hosts.
//!
//! Each command here reads. None of them changes the drive.

use onyx_core::{Error, Result};

pub use crate::sat::{ATA_IDENTIFY, ATA_SMART_READ_DATA, ATA_SMART_READ_THRESHOLDS, AtaCommand};

/// `IOCTL_STORAGE_QUERY_PROPERTY`.
pub const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;

/// `IOCTL_ATA_PASS_THROUGH`.
pub const IOCTL_ATA_PASS_THROUGH: u32 = 0x0004_D02C;

/// `StorageAdapterProtocolSpecificProperty`. Identify goes to the adapter.
const ADAPTER_PROTOCOL_SPECIFIC: u32 = 49;

/// `StorageDeviceProtocolSpecificProperty`. A log page goes to the device.
const DEVICE_PROTOCOL_SPECIFIC: u32 = 50;

/// `ProtocolTypeNvme`.
const PROTOCOL_NVME: u32 = 3;

/// `NVMeDataTypeIdentify`.
const NVME_DATA_IDENTIFY: u32 = 1;

/// `NVMeDataTypeLogPage`.
const NVME_DATA_LOG_PAGE: u32 = 2;

/// The size of `STORAGE_PROTOCOL_SPECIFIC_DATA`, in bytes.
const SPECIFIC_BYTES: usize = 40;

/// Where `STORAGE_PROTOCOL_SPECIFIC_DATA` starts in the query and in the
/// answer: after `PropertyId` and `QueryType`, or after `Version` and `Size`.
const SPECIFIC_AT: usize = 8;

/// One NVMe request through the protocol-specific property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvmeQuery {
    property: u32,
    data_type: u32,
    request: u32,
    sub_request: u32,
    pub length: u32,
}

/// Identify Controller: CNS 1.
pub const NVME_IDENTIFY_CONTROLLER: NvmeQuery = NvmeQuery {
    property: ADAPTER_PROTOCOL_SPECIFIC,
    data_type: NVME_DATA_IDENTIFY,
    request: 1,
    sub_request: 0,
    length: 4096,
};

/// The SMART / Health Information log, log page 2.
pub const NVME_HEALTH_LOG: NvmeQuery = NvmeQuery {
    property: DEVICE_PROTOCOL_SPECIFIC,
    data_type: NVME_DATA_LOG_PAGE,
    request: 2,
    sub_request: 0,
    length: 512,
};

/// The buffer for one query. The answer comes back in the same buffer.
///
/// The layout: `STORAGE_PROPERTY_QUERY` with its `PropertyId` and its
/// `QueryType`, then `STORAGE_PROTOCOL_SPECIFIC_DATA` as its additional
/// parameters, then room for the data.
pub fn nvme_query_buffer(query: NvmeQuery) -> Vec<u8> {
    let mut out = vec![0u8; SPECIFIC_AT + SPECIFIC_BYTES + query.length as usize];
    let mut put = |at: usize, value: u32| out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(0, query.property);
    put(4, 0); // PropertyStandardQuery
    put(SPECIFIC_AT, PROTOCOL_NVME);
    put(SPECIFIC_AT + 4, query.data_type);
    put(SPECIFIC_AT + 8, query.request);
    put(SPECIFIC_AT + 12, query.sub_request);
    put(SPECIFIC_AT + 16, SPECIFIC_BYTES as u32); // ProtocolDataOffset
    put(SPECIFIC_AT + 20, query.length); // ProtocolDataLength
    out
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read the data out of the answer, `STORAGE_PROTOCOL_DATA_DESCRIPTOR`.
///
/// The descriptor gives the offset of the data from the start of its
/// protocol-specific part, and the length. A length or an offset that does
/// not fit the buffer is an error and not a read outside it.
pub fn nvme_answer(answer: &[u8], want: u32) -> Result<Vec<u8>> {
    let offset = u32_at(answer, SPECIFIC_AT + 16);
    let length = u32_at(answer, SPECIFIC_AT + 20);
    let (Some(offset), Some(length)) = (offset, length) else {
        return Err(Error::host(
            "STORAGE_PROTOCOL_DATA_DESCRIPTOR",
            "the answer is too short",
        ));
    };
    if length < want {
        return Err(Error::host(
            "STORAGE_PROTOCOL_DATA_DESCRIPTOR",
            format!("{length} bytes of data, and the request was for {want}"),
        ));
    }
    let start = SPECIFIC_AT + offset as usize;
    answer
        .get(start..start + want as usize)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            Error::host(
                "STORAGE_PROTOCOL_DATA_DESCRIPTOR",
                format!("the data at {start} does not fit in {} bytes", answer.len()),
            )
        })
}

/// The size of `ATA_PASS_THROUGH_EX` on a 64-bit host. `DataBufferOffset`
/// is a `ULONG_PTR`, so it sits at byte 24 after four bytes of padding.
pub const ATA_HEADER_BYTES: usize = 48;

/// The size of the data that each ATA command here reads.
const ATA_DATA_BYTES: usize = 512;

/// `ATA_FLAGS_DRDY_REQUIRED | ATA_FLAGS_DATA_IN`.
const ATA_FLAGS: u16 = 0x01 | 0x02;

/// The time that a drive gets to answer, in seconds.
const ATA_TIMEOUT_SECONDS: u32 = 10;

/// The buffer for one ATA command: `ATA_PASS_THROUGH_EX`, then the data.
pub fn ata_buffer(command: AtaCommand) -> Vec<u8> {
    let mut out = vec![0u8; ATA_HEADER_BYTES + ATA_DATA_BYTES];
    out[0..2].copy_from_slice(&(ATA_HEADER_BYTES as u16).to_le_bytes());
    out[2..4].copy_from_slice(&ATA_FLAGS.to_le_bytes());
    out[8..12].copy_from_slice(&(ATA_DATA_BYTES as u32).to_le_bytes());
    out[12..16].copy_from_slice(&ATA_TIMEOUT_SECONDS.to_le_bytes());
    out[24..32].copy_from_slice(&(ATA_HEADER_BYTES as u64).to_le_bytes());
    // CurrentTaskFile: features, sector count, LBA low, LBA mid, LBA high,
    // device, command, reserved.
    let task = &mut out[40..48];
    task[0] = command.features;
    task[1] = 1;
    task[3] = command.lba_mid;
    task[4] = command.lba_high;
    task[6] = command.command;
    out
}

/// The status register that the drive left in the task file, and the data.
///
/// Bit 0 of the status is ERR. A drive that sets it refused the command.
pub fn ata_answer(answer: &[u8]) -> Option<Vec<u8>> {
    let status = *answer.get(40 + 6)?;
    if status & 0x01 != 0 {
        return None;
    }
    answer
        .get(ATA_HEADER_BYTES..ATA_HEADER_BYTES + ATA_DATA_BYTES)
        .map(<[u8]>::to_vec)
}

/// `IOCTL_SCSI_PASS_THROUGH`.
pub const IOCTL_SCSI_PASS_THROUGH: u32 = 0x0004_D004;

/// The size of `SCSI_PASS_THROUGH` on a 64-bit host. `DataBufferOffset` is a
/// `ULONG_PTR` at byte 24, and the structure ends after the 16 bytes of the
/// command at byte 52, padded to 56.
const SCSI_HEADER_BYTES: usize = 56;

/// Room for the sense data that the device gives back.
const SENSE_BYTES: usize = 32;

/// Where the data starts: after the header and the sense data.
const SCSI_DATA_AT: usize = SCSI_HEADER_BYTES + SENSE_BYTES;

/// `SCSI_IOCTL_DATA_IN`.
const SCSI_DATA_IN: u8 = 1;

/// The buffer for one SCSI command that reads 512 bytes: `SCSI_PASS_THROUGH`,
/// the sense data, then the data.
///
/// This carries ATA PASS-THROUGH to a drive behind a USB bridge, because the
/// ATA control code reaches no drive on the USB storage driver.
pub fn scsi_buffer(cdb: [u8; 16]) -> Vec<u8> {
    let mut out = vec![0u8; SCSI_DATA_AT + ATA_DATA_BYTES];
    out[0..2].copy_from_slice(&(SCSI_HEADER_BYTES as u16).to_le_bytes());
    out[6] = cdb.len() as u8;
    out[7] = SENSE_BYTES as u8;
    out[8] = SCSI_DATA_IN;
    out[12..16].copy_from_slice(&(ATA_DATA_BYTES as u32).to_le_bytes());
    out[16..20].copy_from_slice(&ATA_TIMEOUT_SECONDS.to_le_bytes());
    out[24..32].copy_from_slice(&(SCSI_DATA_AT as u64).to_le_bytes());
    out[32..36].copy_from_slice(&(SCSI_HEADER_BYTES as u32).to_le_bytes());
    out[36..52].copy_from_slice(&cdb);
    out
}

/// The data of a SCSI answer, when the device gave a good status.
///
/// A status of CHECK CONDITION means the bridge or the drive refused the
/// command.
pub fn scsi_answer(answer: &[u8]) -> Option<Vec<u8>> {
    if *answer.get(2)? != 0 {
        return None;
    }
    answer
        .get(SCSI_DATA_AT..SCSI_DATA_AT + ATA_DATA_BYTES)
        .map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_asks_the_adapter_for_cns_one() {
        let b = nvme_query_buffer(NVME_IDENTIFY_CONTROLLER);
        assert_eq!(b.len(), 8 + 40 + 4096);
        assert_eq!(u32_at(&b, 0), Some(49));
        assert_eq!(u32_at(&b, 4), Some(0));
        assert_eq!(u32_at(&b, 8), Some(3));
        assert_eq!(u32_at(&b, 12), Some(1));
        assert_eq!(u32_at(&b, 16), Some(1));
        assert_eq!(u32_at(&b, 24), Some(40));
        assert_eq!(u32_at(&b, 28), Some(4096));
    }

    #[test]
    fn the_health_log_asks_the_device_for_log_two() {
        let b = nvme_query_buffer(NVME_HEALTH_LOG);
        assert_eq!(u32_at(&b, 0), Some(50));
        assert_eq!(u32_at(&b, 12), Some(2));
        assert_eq!(u32_at(&b, 16), Some(2));
        assert_eq!(u32_at(&b, 28), Some(512));
    }

    /// An answer as Windows writes it: Version and Size, the specific data
    /// with the offset and the length, and then the data.
    fn answer(offset: u32, length: u32, data: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 8 + offset as usize + data.len()];
        b[24..28].copy_from_slice(&offset.to_le_bytes());
        b[28..32].copy_from_slice(&length.to_le_bytes());
        b[8 + offset as usize..].copy_from_slice(data);
        b
    }

    #[test]
    fn the_data_comes_from_the_offset_that_the_answer_gives() {
        let data: Vec<u8> = (0..=255).cycle().take(512).collect();
        let b = answer(40, 512, &data);
        assert_eq!(nvme_answer(&b, 512).unwrap(), data);
    }

    #[test]
    fn an_answer_shorter_than_the_request_is_an_error() {
        let b = answer(40, 256, &[0u8; 256]);
        assert!(nvme_answer(&b, 512).is_err());
    }

    #[test]
    fn an_offset_past_the_end_is_an_error_and_not_a_panic() {
        let mut b = answer(40, 512, &[0u8; 512]);
        b[24..28].copy_from_slice(&4000u32.to_le_bytes());
        assert!(nvme_answer(&b, 512).is_err());
        assert!(nvme_answer(&[0u8; 10], 512).is_err());
    }

    #[test]
    fn the_ata_header_points_at_the_data_after_it() {
        let b = ata_buffer(ATA_SMART_READ_DATA);
        assert_eq!(b.len(), 48 + 512);
        assert_eq!(&b[0..2], &48u16.to_le_bytes());
        assert_eq!(&b[2..4], &3u16.to_le_bytes());
        assert_eq!(&b[8..12], &512u32.to_le_bytes());
        assert_eq!(&b[24..32], &48u64.to_le_bytes());
        assert_eq!(&b[40..47], &[0xd0, 1, 0, 0x4f, 0xc2, 0, 0xb0]);
    }

    #[test]
    fn the_scsi_header_carries_the_command_and_points_past_the_sense_data() {
        let cdb = crate::sat::ata_pass_through_16(ATA_IDENTIFY);
        let b = scsi_buffer(cdb);
        assert_eq!(b.len(), 56 + 32 + 512);
        assert_eq!(&b[0..2], &56u16.to_le_bytes());
        assert_eq!((b[6], b[7], b[8]), (16, 32, 1));
        assert_eq!(&b[12..16], &512u32.to_le_bytes());
        assert_eq!(&b[24..32], &88u64.to_le_bytes());
        assert_eq!(&b[32..36], &56u32.to_le_bytes());
        assert_eq!(&b[36..52], &cdb);
    }

    #[test]
    fn a_scsi_answer_with_a_bad_status_gives_no_data() {
        let mut b = scsi_buffer([0; 16]);
        b[88] = 0x42;
        assert_eq!(scsi_answer(&b).unwrap()[0], 0x42);
        b[2] = 0x02; // CHECK CONDITION
        assert!(scsi_answer(&b).is_none());
        assert!(scsi_answer(&[]).is_none());
    }

    #[test]
    fn an_ata_answer_with_the_error_bit_gives_no_data() {
        let mut b = ata_buffer(ATA_IDENTIFY);
        b[48] = 0x55;
        b[46] = 0x50; // DRDY and DSC, no error
        assert_eq!(ata_answer(&b).unwrap()[0], 0x55);
        b[46] = 0x51; // ERR
        assert!(ata_answer(&b).is_none());
    }
}
