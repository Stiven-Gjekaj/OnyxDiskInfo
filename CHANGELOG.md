<div align="center">
  <a href="README.md"><img src="assets/logo.svg" alt="OnyxDiskInfo" height="44"></a>
</div>

# Changelog

Every release is written here, newest first. A version is `MAJOR.MINOR.PATCH`.

## 0.2.0 - 2026-10-16

### Added

- A Debian package and an RPM package of the app and the command line, for
  x86_64 and arm64.

### Fixed

- `onyx bench` runs each read job first, then each write job, as
  CrystalDiskMark does. A read right after a heavy write was slower on some
  drives.

## 0.1.0 - 2026-10-11

The first release.

### Added

- `onyx list` shows each drive on Windows, on macOS, and on Linux: the name,
  the size, the bus, where it sits, the firmware, and the serial number.
  `--json` prints the same list for a script.
- `onyx health <number>` shows the health of one drive: a verdict with its
  reasons, the temperature, the hours, and every counter of the NVMe health
  log or every ATA SMART attribute. It reads NVMe drives on all three hosts,
  and ATA drives through pass-through on Linux and on Windows.
- Each health record names its source and its confidence: the drive, a USB
  bridge, smartctl, or a measurement.
- A drive that reports no health gets a measured record: 64 timed reads
  spread over the whole drive.
- eMMC life estimates on Linux.
- A drive in a USB case answers SAT on Windows, through the SCSI control
  code.
- smartctl is a fallback when it is on the computer.
- `onyx scan` reads every block of a drive and sorts each one by its read
  time.
- `onyx bench` runs the four jobs of CrystalDiskMark on a drive, for reads,
  or in a test file, for reads and writes.
- `onyx test capacity` finds a drive that claims more space than it has, and
  puts back each block that it wrote over.
- `onyx test write` writes every block of a drive and reads each one back.
- A test that writes refuses the system disk and a read-only drive, needs
  `--force` for an internal drive, and asks for the model and the size of
  the drive.
- `OnyxDiskInfo`, a desktop app in the style of CrystalDiskInfo: a tab for
  each drive, the health and the temperature, the details of the drive, and
  the attribute table. It follows the light or the dark scheme of the
  system.
- The desktop app asks for administrator rights when it starts on Windows.
  On macOS and on Linux, a drive that needs root gets a button that reads it
  through the password dialog of the system.
- `onyx format` writes a GPT or an MBR with one partition. Onyx writes
  FAT12, FAT16, FAT32, and exFAT itself, on each host. The tools of the host
  write NTFS, ext4, btrfs, and XFS on Linux, APFS and HFS+ on macOS, and
  NTFS on Windows.
- `onyx erase` writes one pass or more over every byte of a drive and reads
  the last pass back, or sends a TRIM of the whole drive and reads a sample.
- `onyx watch` reads every drive and keeps each reading in a SQLite
  history. A worse verdict, more damage, or more wear makes an alert.
  `onyx history` and `onyx alerts` show what the history holds.
- The desktop app keeps each reading too, charts the temperature of each
  drive, and shows its alerts. A tray icon keeps the app reading the drives
  each hour after the window closes.

### Fixed

- A flush on a drive on macOS failed, so the write test counted a bad block
  and the capacity test stopped. Onyx now sends the request that a disk
  node takes.
- A benchmark job could give a speed of zero when the host started a worker
  late.
- Attributes 241 and 242 are "Total Host Writes" and "Total Host Reads". The
  old names gave a unit, LBAs, and a Kingston SA400 counts GiB there.
