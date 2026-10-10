<div align="center">
  <a href="../README.md"><b>OnyxDiskInfo</b></a>
</div>

# Milestones

Nothing is built yet.
[The roadmap](roadmap.md) says in what order to build Onyx.
This file holds the decisions that shape the work, and the reason behind each
one.
It also holds the options that lost, because the reason a choice lost is the
part that a later reader needs.

---

## What Onyx is

Onyx reads the health of every disk that a computer can see.
It also tests a disk, partitions it, formats it, and erases it.
It runs on Windows, on macOS, and on Linux.
It has a command line, `onyx`, and a desktop app, `OnyxDiskInfo`.

CrystalDiskInfo is the model for the health view.
Onyx goes further in three ways:

1. It runs on all three hosts.
2. It gives a health record to a disk that has no SMART.
3. It tests, formats, and erases a disk.

---

## The language and the toolkit

**Rust, edition 2024, with the toolchain pinned in `rust-toolchain.toml`.**
Burnout, HotDogStand, chrysoberyl, and DeForm6 use Rust.
Burnout already opens raw devices on all three hosts.

**Slint for the desktop app.**
HotDogStand and BetonPlusium use Slint.
A `Theme` global holds every colour and size.

**`clap` for the command line.**
Each command has a `--json` form.
The desktop app and the command line call the same library code.

**`thiserror` in a library, `anyhow` in a binary.**

**Raw `windows-sys`, `libc`, and `io-kit-sys` for the host calls.**
Each one is behind a `cfg` target.

**SQLite through `rusqlite`, bundled, for the history.**

Each dependency has an exact pin, `=x.y.z`.

Options that lost:

- egui and iced. Slint is already known, and the theme pattern exists.
- Tauri. It adds a web stack and a second language.
- A wrapper around smartmontools as the core. See the next section.

---

## The crates

| Crate | Work |
|---|---|
| `onyx-core` | It decodes ATA SMART, the NVMe health log, SCSI log pages, and eMMC EXT_CSD. It scores health. It holds the drive model and the table of vendor and bridge quirks. It takes no dependency. |
| `onyx-probe` | It lists drives and sends pass-through commands on each host. |
| `onyx-bench` | It runs the surface scan, the write and verify test, the fake capacity test, and the benchmarks. |
| `onyx-layout` | It writes GPT and MBR, FAT12, FAT16, FAT32, and exFAT. It erases a disk. It calls the host for the other file systems. |
| `onyx-store` | It keeps the history in SQLite. |
| `onyx` | The command line. |
| `onyx-gui` | The desktop app. |

`onyx-core` takes no dependency, so a test gives it raw bytes from a real
disk.
A test does not need the disk.

---

## Where the health data comes from

Onyx uses the best source that a disk has.
It tries them in this order:

1. Native ATA SMART or the NVMe health log.
2. A USB bridge: SAT pass-through, then the vendor commands for JMicron,
   ASMedia, and Realtek.
3. eMMC EXT_CSD life estimates, and the SD card health register where a card
   has one.
4. `smartctl --json`, if `smartctl` is on the `PATH`.
   This step is for RAID controllers and rare bridges that the native code
   cannot reach.
5. Onyx measures the disk itself.

A disk with no health data still gets a record.
Today the record is a sample of 64 reads of 1 MiB, spread from the start to
the end of the drive, each one timed.
A failed read is a bad verdict, and a read of a second or more is a caution.
P4 adds the surface scan, the fake capacity result, and the benchmark trend.

Each health result has a confidence level.
The app shows it.
Reason: a value that Onyx calculates from its own tests is not a value that
the disk reports.
The two must not look the same.

**The vendor commands of USB bridges wait for hardware.**
Most external SSDs today are NVMe drives behind a JMicron JMS583, an ASMedia
ASM2362, or a Realtek RTL9210.
Each of these takes its own SCSI command, and the layouts are not public
standards.
Onyx sends no vendor command that nobody has tested on the chip it is for,
because a wrong command to an unknown chip can hang the drive.
Until a bridge of each kind is at hand, `smartctl` reads them, and the record
says so with a medium confidence.
This decision was made on 2026-10-10.

**The SD card health register is not a standard.**
SD cards have no health data in the SD standard.
Some industrial cards give it through a vendor command, CMD56, with a layout
that each maker defines.
Onyx measures an SD card instead.

**`smartctl` is a fallback, not the core.**
Onyx runs it as a separate program and reads its JSON.
Onyx links no GPL code, so the licence stays MIT.

Options that lost:

- Native only. Some RAID controllers and rare bridges stay unreadable.
- `smartctl` as the core. Onyx then needs a second program on each host, and
  a disk without SMART still gets nothing.

---

## Formatting

**Onyx writes GPT, MBR, FAT12, FAT16, FAT32, and exFAT itself.**
These are the file systems of removable disks.
A test writes them to a file and reads them back.
The result is the same on all three hosts.

**Onyx calls the host for the other file systems.**
These are NTFS, ext4, btrfs, XFS, APFS, and HFS+.
The host tools are `mkfs.*` on Linux, `diskutil` and `newfs_*` on macOS, and
`format` on Windows.
A table in `onyx-layout` says which file system each host can write.
The app shows only the file systems that the host can write.

This is different from Burnout.
Burnout never calls the host.
Burnout writes a fixed layout for one purpose.
Onyx writes many file systems, and some of them take months to write in Rust.

Options that lost:

- Write NTFS and ext4 in Rust too. This is months of extra work.
- Use the host tools for everything. The set of file systems then changes
  from host to host, and the common ones cannot be tested on a file.

---

## Erasing

Onyx uses the command that the disk gives for erasing.
These are ATA Secure Erase, NVMe Format, and NVMe Sanitize.
If the disk has none, Onyx uses a full TRIM or it writes passes over the
disk.
Each erase reads a sample back and shows the result.

---

## The order of the work

**The command line comes first. The desktop app comes in phase 5.**
The command line proves each host backend on all three hosts.
The app is then a view over code that works.

---

## Who can open a disk

Each host needs administrator rights to send pass-through commands.
The command line runs with `sudo` or from an administrator shell.

**The desktop app runs as the person, and one read runs as root.**
This decision was made on 2026-10-10.

- On Windows, the app asks for administrator rights when it starts, through
  its manifest, as CrystalDiskInfo does. Each health command on Windows needs
  them, so an app without them shows nothing.
- On macOS and on Linux, the app runs as the person who starts it. macOS
  reads an NVMe drive with no rights at all. A drive that needs root gets a
  button. The button runs `onyx health <number> --json` through the password
  dialog of the system, `osascript` on macOS and `pkexec` on Linux, and the
  app reads the JSON that comes back.

The app builds its view from the JSON of `onyx health --json`, for a drive
that it reads itself and for one that the command line reads with rights. So
the app cannot show other values than the command line prints.

Options that lost:

- Run the whole app as root on macOS and on Linux. A window that runs as root
  reads and writes the files of root, and the desktop of the person does not
  reach it.
- A helper that stays running as root. It is a second program that listens,
  and a bug in it gives root to anybody who reaches it.

---

## The repository

- Public on GitHub: `Stiven-Gjekaj/OnyxDiskInfo`.
- MIT licence.
- The command line is `onyx`. The desktop app is `OnyxDiskInfo`.
- Releases go to the Homebrew tap, the Scoop bucket, and winget, like
  Burnout.
