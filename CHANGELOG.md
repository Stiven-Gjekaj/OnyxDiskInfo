<div align="center">
  <a href="README.md"><img src="assets/logo.svg" alt="OnyxDiskInfo" height="44"></a>
</div>

# Changelog

Every release is written here, newest first. A version is `MAJOR.MINOR.PATCH`.

## Unreleased

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
