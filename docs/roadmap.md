<div align="center">
  <a href="../README.md"><b>OnyxDiskInfo</b></a>
</div>

# The roadmap

[docs/milestones.md](milestones.md) says what Onyx does and why.
This file says in what order to build it.

Each phase has a goal, the work in it, and an exit test.
Do not start a phase until the phase it depends on passes its exit test.
Each phase is many small commits.

Nothing is built yet. P0 is next.

```mermaid
flowchart LR
    P0[P0 scaffold] --> P1[P1 list drives]
    P1 --> P2[P2 ATA and NVMe health]
    P2 --> P3[P3 disks without SMART]
    P3 --> P4[P4 disk tests]
    P2 --> P5[P5 desktop app]
    P4 --> P6[P6 partition, format, erase]
    P5 --> P7[P7 history and alerts]
    P6 --> P8[P8 release]
    P7 --> P8
```

---

## P0: the scaffold

Work:

- The workspace, `rust-toolchain.toml`, the lints, `clippy.toml`, and
  `rustfmt.toml`.
- README, CHANGELOG, CODE_OF_CONDUCT, CONTRIBUTING, SECURITY, SUPPORT,
  TERMS, and LICENSE.
- `.github`: CI, CODEOWNERS, FUNDING, and the issue and pull request
  templates.
- The `.githooks/pre-commit` hook.
- The public repository on GitHub.

Exit test: CI passes on Windows, on macOS, and on Linux.

## P1: list the drives

Work: `onyx list` shows each drive.
It shows the model, the serial number, the firmware, the size, the bus, and
the transport.
It covers internal, USB, SD, and NVMe drives.

Exit test: on each host, `onyx list --json` agrees with the host tool.
The host tools are `diskutil list`, `lsblk`, and `Get-PhysicalDisk`.

## P2: ATA and NVMe health

Work: `onyx-core` decodes ATA SMART attributes, thresholds, and the self
test log.
It also decodes the NVMe health log.
`onyx health <drive>` shows them with a health score and a temperature.

Exit test: on each host, the values agree with `smartctl -a` for one SATA
drive and one NVMe drive.
A test decodes a raw byte dump from each drive.

## P3: disks without SMART

Work:

- SAT pass-through, and the JMicron, ASMedia, and Realtek bridge commands.
- eMMC EXT_CSD and the SD health register.
- The `smartctl --json` fallback.
- A confidence level on each health result.

Exit test: a SATA drive in a USB case shows its SMART.
A USB flash drive with no SMART shows a record with a low confidence level.

## P4: disk tests

Work:

- A read only surface scan with a map of the slow and the bad blocks.
- A write and verify test.
- A fake capacity test.
- Sequential, random, QD1, and QD32 benchmarks.

Exit test: the fake capacity test finds a disk image that claims more space
than it has.
The benchmark numbers on one drive are within 10 percent of CrystalDiskMark
or `fio`.

## P5: the desktop app

Work: a Slint app with one tab for each drive.
It shows the health badge, the temperature, and the attribute table, like
CrystalDiskInfo.
It also decides how the app gets administrator rights.

Exit test: the app shows the same values as `onyx health` on all three hosts.

## P6: partition, format, and erase

Work:

- GPT and MBR.
- FAT12, FAT16, FAT32, and exFAT, written by Onyx.
- NTFS, ext4, btrfs, XFS, APFS, and HFS+, written by the host tools.
- ATA Secure Erase, NVMe Format, NVMe Sanitize, a full TRIM, and overwrite
  passes.

Exit test: each host mounts each file system that Onyx writes.
A file system that Onyx writes passes `fsck` on Linux.

## P7: history and alerts

Work: SQLite history, trend charts, alerts when a value changes, and a tray
icon.

Exit test: a value that changes between two polls makes an alert.

## P8: the release

Work: the release workflow, the build attestation, and the Homebrew, Scoop,
and winget manifests.

Exit test: each package manager installs the release on a clean host.
