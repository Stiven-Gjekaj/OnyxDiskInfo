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
