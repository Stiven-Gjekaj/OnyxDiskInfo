//! The volumes of a Windows disk, which a test that writes locks first.
//!
//! This module makes no system call and names no Windows type. It comes from
//! Burnout, which locks the volumes of a drive before it writes the drive.

use super::parse::disk_extents;

/// One volume, as the host answered about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawVolume {
    /// The name that `FindFirstVolumeW` gave, with its trailing backslash.
    pub guid_path: String,
    /// The answer to `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`.
    ///
    /// A card reader with no card gives none, and that is not a failure.
    pub extents: Option<Vec<u8>>,
}

/// The volumes that sit on one disk, ready to open.
///
/// A drive is not written while a file system holds its volumes, so a test
/// that writes locks every volume of the drive first. This says which ones
/// they are.
///
/// The paths come back without the trailing backslash that
/// `FindFirstVolumeW` puts on the end. That one character is the whole
/// difference between a handle and an error.
pub fn volumes_on_disk(volumes: &[RawVolume], disk: u32) -> Vec<String> {
    let mut out = Vec::new();
    for volume in volumes {
        let Some(bytes) = volume.extents.as_deref() else {
            continue;
        };
        let Ok(disks) = disk_extents(bytes) else {
            continue;
        };
        if !disks.contains(&disk) {
            continue;
        }
        if let Some(path) = volume_path_to_open(&volume.guid_path) {
            out.push(path);
        }
    }
    out
}

/// Turn the name of a volume into the name that opens it.
///
/// `FindFirstVolumeW` gives a name that ends in a backslash, and `CreateFileW`
/// refuses that name and takes the one without it. A name that does not look
/// like a volume is dropped rather than trimmed into something else.
pub fn volume_path_to_open(guid_path: &str) -> Option<String> {
    let trimmed = guid_path.strip_suffix('\\').unwrap_or(guid_path);
    if !trimmed.starts_with(r"\\?\") || trimmed.len() <= r"\\?\".len() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extents(disks: &[u32]) -> Vec<u8> {
        let mut bytes = vec![0u8; 8 + disks.len() * 24];
        bytes[0..4].copy_from_slice(&(disks.len() as u32).to_le_bytes());
        for (index, disk) in disks.iter().enumerate() {
            let at = 8 + index * 24;
            bytes[at..at + 4].copy_from_slice(&disk.to_le_bytes());
        }
        bytes
    }

    fn volume(path: &str, disks: &[u32]) -> RawVolume {
        RawVolume {
            guid_path: path.to_string(),
            extents: Some(extents(disks)),
        }
    }

    const ON_DISK_0: &str = r"\\?\Volume{11111111-1111-1111-1111-111111111111}\";
    const ON_DISK_2: &str = r"\\?\Volume{22222222-2222-2222-2222-222222222222}\";

    #[test]
    fn only_the_volumes_of_the_named_disk_come_back() {
        // A lock on the wrong volume takes the system disk away from the
        // running machine, so this is the test that matters most here.
        let volumes = [volume(ON_DISK_0, &[0]), volume(ON_DISK_2, &[2])];
        let found = volumes_on_disk(&volumes, 2);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("22222222"));
        assert!(volumes_on_disk(&volumes, 1).is_empty());
    }

    #[test]
    fn a_volume_that_spans_two_disks_belongs_to_both() {
        let volumes = [volume(ON_DISK_0, &[0, 2])];
        assert_eq!(volumes_on_disk(&volumes, 0).len(), 1);
        assert_eq!(volumes_on_disk(&volumes, 2).len(), 1);
    }

    #[test]
    fn a_volume_that_answered_nothing_is_dropped_and_does_not_fail_the_list() {
        // An empty card reader answers nothing. Failing here would stop a
        // write to a drive that has nothing to do with that reader.
        let volumes = [
            RawVolume {
                guid_path: ON_DISK_0.to_string(),
                extents: None,
            },
            volume(ON_DISK_2, &[2]),
        ];
        assert_eq!(volumes_on_disk(&volumes, 2).len(), 1);
    }

    #[test]
    fn the_trailing_backslash_comes_off_the_volume_name() {
        // CreateFileW refuses the name that FindFirstVolumeW gives, and takes
        // the same name without its last character.
        assert_eq!(
            volume_path_to_open(ON_DISK_0).unwrap(),
            r"\\?\Volume{11111111-1111-1111-1111-111111111111}"
        );
    }

    #[test]
    fn a_name_that_is_not_a_volume_is_dropped_and_not_trimmed() {
        assert!(volume_path_to_open("").is_none());
        assert!(volume_path_to_open(r"\\?\").is_none());
        assert!(volume_path_to_open(r"C:\").is_none());
    }
}
