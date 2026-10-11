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
| `onyx-probe` | It lists drives and sends pass-through commands on each host. It sends the TRIM of an erase. |
| `onyx-bench` | It runs the surface scan, the write and verify test, the fake capacity test, the benchmarks, and the overwrite passes of an erase. |
| `onyx-layout` | It writes GPT and MBR, FAT12, FAT16, FAT32, and exFAT. It gives the command line of the host tool for the other file systems. |
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
A table in `onyx-layout` says which file system each host can write:

| Host | Tool | File systems |
|---|---|---|
| Linux | Onyx writes the table, then `mkfs.*` writes the partition | NTFS, ext4, btrfs, XFS |
| macOS | `diskutil eraseDisk` writes both | APFS, HFS+ |
| Windows | `Clear-Disk`, `Initialize-Disk`, `New-Partition`, `Format-Volume` | NTFS |

A host with no writer for a file system refuses it before the prompt, and
names the hosts that have one.
The app shows only the file systems that the host can write.

On macOS and Windows, the host tool writes the table too. APFS needs the
container that `diskutil` makes, and Windows puts its own reserved partition
on a GPT. A table that Onyx writes first would only be written over.

**MBR is the default table.** Each camera, car radio, and console reads MBR,
and many read no GPT. `--table gpt` gives a GPT. APFS needs a GPT, and Onyx
refuses APFS on an MBR.

**FAT12 and FAT16 come from the `fatfs` crate, after a dry run.** `fatfs`
takes the FAT type as a hint for the size of a cluster, and then writes the
type that the count of clusters gives. A FAT12 of 1 GiB came out as FAT16.
Onyx formats a disk that keeps only its first sector first, reads the type
from it, and refuses a size that gives another type before it writes a byte.
`fatfs` also leaves the count of hidden sectors at zero, and Windows refuses
such a volume, so Onyx writes the start of the partition there.

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

**The overwrite passes and the full TRIM come first. The commands of the
firmware wait for a drive to test them.** Decided on 2026-10-11.
An overwrite and a TRIM can be tested on a disk image that each host
attaches, and CI does that. A firmware erase cannot: it goes to a real
drive, it cannot be stopped, and a wrong command can lock a drive with a
password or leave it unusable. The only NVMe drive at hand is the system
disk, which Onyx refuses, and no SATA drive is at hand. So the firmware
erases wait, as the SATA half of P2 does.

- One pass of zeros is the default. It is the "clear" of NIST SP 800-88 for
  a disk that a person cannot read past its own interface.
- The last pass reads back unless `--no-check` is given.
- A TRIM is not an erase that a person can prove. A drive may give back the
  old bytes of a trimmed block until it erases the block. Onyx reads 64
  blocks spread over the drive after the TRIM and prints how many hold only
  zeros. The disk images of macOS and Windows take the TRIM and keep every
  byte, and Onyx says so.

---

## The history and the alerts

**A reading is a flat list of names and numbers.** `onyx-core` turns each
health record into it, for NVMe, ATA, eMMC, and a measured drive alike. The
history keeps the list, and the alert rules compare two lists. Neither needs
to know a protocol. The names are part of the file format: a name never
changes, and a new value gets a new name.

**A drive is known by its model and its serial number.** Its number in the
list and its node change when it moves to another port. A drive with no
serial number is known by its node.

**An alert says that something changed that a person must know.** It is
not a change of any value:

- The verdict got worse. Bad is critical, and Caution is a warning.
- A count of damage went up: reallocated, pending, and uncorrectable
  sectors, CRC errors, program and erase fails, NVMe media errors, minutes
  above the temperature limits of the drive, failed reads of a measured
  drive, and the end-of-life state of an eMMC chip.
- The drive set a new bit of the NVMe critical warning.
- The wear passed 50, 80, 90, or 100 percent.

The temperature, the hours, and the data written move all the time. They
make no alert, and the history keeps them for the charts.

**The history file is in the data folder of the person.** That is
`~/Library/Application Support/OnyxDiskInfo` on macOS, `%LOCALAPPDATA%` on
Windows, and `$XDG_DATA_HOME` or `~/.local/share` on Linux. `sudo` can give
root a home of its own, so a command that runs with `sudo` takes `--db` to
name the same file.

**SQLite is built in on each host.** The checks of the other targets in CI
compile no C, so they link a system SQLite, which a check never links.

**The tray comes from Slint.** Slint 1.18 has `SystemTrayIcon` on all three
hosts. With a tray, a closed window hides, and the app reads the drives each
hour. A test with no display has no tray.

Options that lost:

- An alert for each change. The temperature changes at each reading, and a
  person soon stops reading alerts.
- A table of columns for each protocol. Each new value would change the
  schema.

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
