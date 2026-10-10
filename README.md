<div align="center">

<img src="assets/logo.svg" alt="OnyxDiskInfo" height="120">

# OnyxDiskInfo

### Reads the health of every disk, and tests, formats and erases it

_One tool, the same on Windows, on macOS, and on Linux._

![Rust](https://img.shields.io/badge/rust-09090b?style=for-the-badge&logo=rust&logoColor=f4f4f5)
![Slint](https://img.shields.io/badge/slint-27272a?style=for-the-badge&logo=slint&logoColor=f4f4f5)
![Windows](https://img.shields.io/badge/windows-3f3f46?style=for-the-badge&logo=windows&logoColor=f4f4f5)
![macOS](https://img.shields.io/badge/macos-71717a?style=for-the-badge&logo=apple&logoColor=f4f4f5)
![Linux](https://img.shields.io/badge/linux-a1a1aa?style=for-the-badge&logo=linux&logoColor=09090b)
[![MIT licence](https://img.shields.io/badge/mit_licence-e4e4e7?style=for-the-badge&logoColor=09090b)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/Stiven-Gjekaj/OnyxDiskInfo/ci.yml?branch=main&style=for-the-badge&label=ci&labelColor=f4f4f5&color=f4f4f5)](https://github.com/Stiven-Gjekaj/OnyxDiskInfo/actions/workflows/ci.yml)

<p align="center">
  <a href="#overview"><b>Overview</b></a> |
  <a href="#screenshots"><b>Screenshots</b></a> |
  <a href="#what-it-does"><b>Features</b></a> |
  <a href="#disks-without-smart"><b>Without SMART</b></a> |
  <a href="#the-shape-of-the-command"><b>Commands</b></a> |
  <a href="docs/roadmap.md"><b>Roadmap</b></a>
</p>

</div>

---

> [!NOTE]
> **The command line and the desktop app work.**
> `onyx list` shows each drive on Windows, on macOS, and on Linux.
> `onyx health` reads an NVMe drive on all three hosts, and agrees with
> smartctl. A drive that reports nothing gets a measured record with a low
> confidence. The SATA and USB bridge paths are written and wait for a drive
> to test them.
> `onyx scan`, `onyx bench`, and `onyx test` work. The capacity test finds a
> fake drive. The comparison of the benchmarks with fio is partly proven.
> The desktop app shows the same values as `onyx health` on all three hosts.
> [docs/roadmap.md](docs/roadmap.md) says what comes next, and
> [docs/milestones.md](docs/milestones.md) holds every decision and the reason
> behind it.

## Overview

OnyxDiskInfo reads what a disk says about its own health.
It reads SATA, NVMe, USB, SD, and eMMC disks.
It also tests a disk, partitions it, formats it, and erases it.

It has two faces that share one library:

- `onyx`, a command line that gives each result as text or as JSON.
- `OnyxDiskInfo`, a desktop app in the style of CrystalDiskInfo.

## Screenshots

The desktop app on the internal SSD of a MacBook Air. It follows the light or
the dark scheme of the system. The serial number is masked.

<p align="center">
  <img src="assets/screenshots/onyx-light.png" alt="OnyxDiskInfo in the light scheme" width="49%">
  <img src="assets/screenshots/onyx-dark.png" alt="OnyxDiskInfo in the dark scheme" width="49%">
</p>

## What it does

| Area | Work |
|---|---|
| Health | ATA SMART, the NVMe health log, SCSI log pages, eMMC life estimates, the temperature, and a health score. |
| Bridges | SAT pass-through for a drive in a USB case. `smartctl` is a fallback if it is on your computer, and it reads the JMicron, ASMedia, and Realtek bridges until Onyx can test its own commands for them. |
| Tests | A surface scan, a write and verify test, a fake capacity test, and benchmarks. |
| Layout | GPT and MBR. Onyx writes FAT12, FAT16, FAT32, and exFAT itself. The tools of the host write NTFS, ext4, btrfs, XFS, APFS, and HFS+. |
| Erase | ATA Secure Erase, NVMe Format, NVMe Sanitize, a full TRIM, and overwrite passes. |
| History | Each reading goes into SQLite. A value that changes makes an alert. |

## Disks without SMART

A cheap flash drive, a card reader, or a virtual disk reports no health.
Onyx still gives it a record.
The record holds the surface scan errors, the read latency over time, the fake
capacity result, and the benchmark trend.

Each health result has a confidence level.
A value that Onyx measures never looks like a value that the disk reports.

## The shape of the command

Every command but `format` and `erase` exists today.

```
onyx list
onyx health <number> [--json]
onyx scan <number> [--json]
onyx bench <number> [--json]
onyx bench --file <path> [--size-mib 1024] [--json]
onyx test capacity <number> [--force] [--confirm "<model> <size>"]
onyx test write <number> [--force] [--confirm "<model> <size>"]
onyx format <number> --fs exfat
onyx erase <number>
```

`scan` reads every block and sorts each one by its read time, like MHDD and
Victoria. `bench` runs the four jobs of CrystalDiskMark: on a drive it only
reads, and in a test file it reads and writes, then deletes the file.
`test capacity` finds a fake drive that claims more space than it has. It
saves the blocks that it writes over and puts them back. `test write` writes
every block and reads each one back, and it erases the drive.

A command that reads never writes.
A command that writes asks you to confirm the drive, and it refuses the disk
that the running system starts from.

## Build

```bash
cargo build --release
```

The toolchain version is in [rust-toolchain.toml](rust-toolchain.toml).
[CONTRIBUTING.md](CONTRIBUTING.md) holds the checks that a change must pass.

## Not affiliated

OnyxDiskInfo has no connection to CrystalDiskInfo or to its author.
CrystalDiskInfo is the model for the health view, and nothing more.

## Support

If you find OnyxDiskInfo useful, you can support its development here.

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/U1G623RXOE)

## License

MIT. See [LICENSE](LICENSE) and [TERMS.md](TERMS.md).
