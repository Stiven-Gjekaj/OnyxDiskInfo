"""Format images with `onyx format`, and let the host check and mount each one.

This is the exit test of P6 for `onyx format`. For each table, GPT and MBR,
and each file system that this host can write, the script:

1. makes an empty image and formats it with `onyx format`,
2. attaches it as a disk with the tool of the host,
3. checks the partition with the check tool of the host,
4. mounts it, writes a file, reads the file back, and checks it again,
5. compares the file system and the label that the host reports.

    sudo python3 scripts/check-format.py target/debug/onyx        # Linux
    python3 scripts/check-format.py target/debug/onyx             # macOS
    python scripts/check-format.py target/debug/onyx.exe          # Windows

Onyx writes FAT12, FAT16, FAT32, and exFAT on each host. A tool of the host
writes the others: NTFS, ext4, btrfs, and XFS on Linux, APFS and HFS+ on
macOS, and NTFS on Windows.

Linux attaches with losetup and needs root, dosfstools, exfatprogs, ntfs-3g,
btrfs-progs, and xfsprogs. macOS attaches with hdiutil. Windows mounts a VHD,
which is the image with a footer of 512 bytes after it, checks with chkdsk,
and needs an Administrator. For NTFS on Windows, onyx formats the VHD as a
drive, by the number that `onyx list` gives it.

The script writes only to the images that it makes, in a new directory.
"""

import os
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import uuid

MIB = 1024 * 1024

# Each file system, the size of its image, the tables to try, and the name
# that each host gives it: the blkid TYPE and VERSION on Linux, the
# personality on macOS, and the FileSystem of Get-Volume on Windows.
ONYX = [
    ("fat12", 16, ["mbr", "gpt"], ("vfat", "FAT12"), "MS-DOS FAT12", "FAT"),
    ("fat16", 128, ["mbr", "gpt"], ("vfat", "FAT16"), "MS-DOS FAT16", "FAT"),
    ("fat32", 600, ["mbr", "gpt"], ("vfat", "FAT32"), "MS-DOS FAT32", "FAT32"),
    ("exfat", 64, ["mbr", "gpt"], ("exfat", None), "ExFAT", "exFAT"),
]
# XFS needs 300 MiB at least.
HOST_TOOLS = {
    "Linux": [
        ("ntfs", 320, ["mbr", "gpt"], ("ntfs", None), None, None),
        ("ext4", 320, ["mbr", "gpt"], ("ext4", None), None, None),
        ("btrfs", 320, ["mbr", "gpt"], ("btrfs", None), None, None),
        ("xfs", 320, ["mbr", "gpt"], ("xfs", None), None, None),
    ],
    "Darwin": [
        ("apfs", 256, ["gpt"], None, "APFS", None),
        ("hfsplus", 256, ["mbr", "gpt"], None, "Journaled HFS+", None),
    ],
    "Windows": [
        ("ntfs", 256, ["mbr", "gpt"], None, None, "NTFS"),
    ],
}
# The check tool of each file system on Linux. Each one only reads.
LINUX_CHECK = {
    "vfat": ["fsck.fat", "-n"],
    "exfat": ["fsck.exfat", "-n"],
    "ntfs": ["ntfsfix", "--no-action"],
    "ext4": ["fsck.ext4", "-n", "-f"],
    "btrfs": ["btrfs", "check", "--readonly"],
    "xfs": ["xfs_repair", "-n"],
}
TEXT = "Onyx wrote this volume, and the host reads it back.\n"


def label_of(fs):
    """The label that the script gives a volume: 11 characters at most, which
    each file system keeps."""
    return f"ONYX {fs.upper()}"[:11]


def run(command, **kwargs):
    """Run a command, and stop the script with its output when it fails."""
    done = subprocess.run(command, capture_output=True, text=True, **kwargs)
    if done.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} gave {done.returncode}:\n"
                           f"{done.stdout}{done.stderr}".strip())
    return done.stdout


def vhd_footer(size):
    """The footer of a fixed VHD of `size` bytes, as the VHD standard gives it."""
    sectors = min(size // 512, 65535 * 16 * 255)
    if sectors >= 65535 * 16 * 63:
        per_track, heads = 255, 16
        cylinders_heads = sectors // per_track
    else:
        per_track = 17
        cylinders_heads = sectors // per_track
        heads = max((cylinders_heads + 1023) // 1024, 4)
        if cylinders_heads >= heads * 1024 or heads > 16:
            per_track, heads = 31, 16
            cylinders_heads = sectors // per_track
        if cylinders_heads >= heads * 1024:
            per_track, heads = 63, 16
            cylinders_heads = sectors // per_track
    cylinders = cylinders_heads // heads
    footer = bytearray(struct.pack(
        ">8sIIQI4sI4sQQHBBII16sB",
        b"conectix", 2, 0x00010000, 0xFFFFFFFFFFFFFFFF, 0, b"onyx", 0x00010000,
        b"Wi2k", size, size, cylinders, heads, per_track, 2, 0, uuid.uuid4().bytes, 0,
    ))
    footer += bytes(512 - len(footer))
    checksum = ~sum(footer) & 0xFFFFFFFF
    footer[64:68] = struct.pack(">I", checksum)
    return bytes(footer)


def make_image(directory, fs, mib, table):
    """An empty image of `mib` MiB. Gives its path and its size."""
    path = os.path.join(directory, f"{fs}-{table}.img")
    with open(path, "wb") as f:
        f.truncate(mib * MIB)
    return path, mib * MIB


def format_image(onyx, path, size, fs, table):
    name = os.path.basename(path)
    run([onyx, "format", "--image", path, "--fs", fs, "--table", table,
         "--label", label_of(fs), "--confirm", f"{name} {size}"])


def write_and_read(mount):
    """Write a file on a mounted volume, and give what reads back."""
    path = os.path.join(mount, "onyx.txt")
    with open(path, "w", newline="\n") as f:
        f.write(TEXT)
    with open(path, newline="\n") as f:
        return f.read()


def check_linux(image, fs, names):
    device = run(["losetup", "--find", "--show", "--partscan", image]).strip()
    part = f"{device}p1"
    mount = tempfile.mkdtemp()
    try:
        # udev can remove the node of a new partition and make it again, so
        # wait until it has handled each event, then for the node.
        subprocess.run(["udevadm", "settle", "--timeout=10"], capture_output=True)
        for _ in range(50):
            if os.path.exists(part):
                break
            time.sleep(0.1)

        # -p reads the volume itself, and not the cache of blkid, which
        # gives no VERSION. blkid gives 2 when it finds nothing: for a tag
        # that the file system does not have, and for a node that udev still
        # makes. So only the type, which each file system has, waits.
        def probe(tag):
            done = subprocess.run(["blkid", "-p", "-o", "value", "-s", tag, part],
                                  capture_output=True, text=True)
            if done.returncode not in (0, 2):
                raise RuntimeError(f"blkid gave {done.returncode}: {done.stderr.strip()}")
            return done.stdout.strip() or None
        kind = None
        for _ in range(10):
            kind = probe("TYPE")
            if kind:
                break
            time.sleep(0.5)
        if not kind:
            raise RuntimeError(f"blkid finds no file system on {part}")
        version, label = probe("VERSION"), probe("LABEL") or ""
        check = LINUX_CHECK.get(kind)
        if check is None:
            raise RuntimeError(f"blkid reads {kind!r}, which this script has no check for")
        run([*check, part])
        run(["mount", "-t", "ntfs-3g" if kind == "ntfs" else kind, part, mount])
        try:
            back = write_and_read(mount)
        finally:
            run(["umount", mount])
        run([*check, part])
        want_kind, want_version = names[0]
        shown = (kind, version if want_version else None)
        return shown == (want_kind, want_version), f"{kind} {version or ''}".strip(), label, back
    finally:
        os.rmdir(mount)
        run(["losetup", "--detach", device])


def hdiutil_detach(disk):
    """Detach a disk image. macOS holds a volume busy for a moment after it
    is unmounted, for Spotlight among others, so try again before force."""
    for _ in range(10):
        if subprocess.run(["hdiutil", "detach", "-quiet", disk]).returncode == 0:
            return
        time.sleep(1)
    run(["hdiutil", "detach", "-force", "-quiet", disk])


def check_macos(image, fs, names):
    attached = run(["hdiutil", "attach", "-imagekey", "diskimage-class=CRawDiskImage",
                    "-nomount", image])
    disk = attached.split()[0]
    # The last node is the volume: the first partition, or the volume in the
    # container that macOS makes for APFS.
    part = attached.strip().splitlines()[-1].split()[0]
    mount = tempfile.mkdtemp()
    try:
        if fs in ("apfs", "hfsplus"):
            check = ["diskutil", "verifyVolume", part]
        else:
            check = ["fsck_exfat" if fs == "exfat" else "fsck_msdos", "-n", part]
        run(check)
        info = run(["diskutil", "info", part])
        fields = dict(line.strip().split(":", 1) for line in info.splitlines() if ":" in line)
        kind = fields.get("File System Personality", "").strip()
        label = fields.get("Volume Name", "").strip()
        run(["diskutil", "mount", "-mountPoint", mount, part])
        try:
            back = write_and_read(mount)
        finally:
            run(["diskutil", "unmount", part])
        run(check)
        return kind == names[1], kind, label, back
    finally:
        os.rmdir(mount)
        hdiutil_detach(disk)


# Check the partition of a mounted VHD that holds the data: the last one,
# after the reserved partition that Windows puts on a GPT.
POWERSHELL_CHECK = r"""
$ErrorActionPreference = 'Stop'
$number = [int]$args[0]
$part = Get-Partition -DiskNumber $number | Where-Object { $_.Type -ne 'Reserved' } | Select-Object -Last 1
if (-not $part.DriveLetter) {
    $part | Add-PartitionAccessPath -AssignDriveLetter
    $part = Get-Partition -DiskNumber $number -PartitionNumber $part.PartitionNumber
}
$root = "$($part.DriveLetter):"
$out = chkdsk $root 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { throw "chkdsk $root gave $LASTEXITCODE`n$out" }
$volume = Get-Volume -DriveLetter $part.DriveLetter
[System.IO.File]::WriteAllText("$root\onyx.txt", $args[1])
$back = [System.IO.File]::ReadAllText("$root\onyx.txt")
$out = chkdsk $root 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { throw "chkdsk $root after the write gave $LASTEXITCODE`n$out" }
@{ fs = $volume.FileSystem; label = $volume.FileSystemLabel; back = $back } | ConvertTo-Json -Compress
"""


def powershell(script, *args):
    with tempfile.NamedTemporaryFile("w", suffix=".ps1", delete=False) as f:
        f.write(script)
    try:
        return run(["pwsh", "-NoProfile", "-File", f.name, *[str(a) for a in args]])
    finally:
        os.remove(f.name)


def as_vhd(image):
    """Give the image the footer of a fixed VHD. Gives the new path."""
    size = os.path.getsize(image)
    vhd = image[:-4] + ".vhd"
    os.replace(image, vhd)
    with open(vhd, "ab") as f:
        f.write(vhd_footer(size))
    return os.path.abspath(vhd)


def mount_vhd(vhd):
    """Mount a VHD for reading and writing, and give its disk number."""
    out = powershell("$ErrorActionPreference = 'Stop'\n"
                     "(Mount-DiskImage -ImagePath $args[0] -Access ReadWrite -PassThru"
                     " | Get-Disk).Number", vhd)
    return int(out.strip().splitlines()[-1])


def check_windows(onyx, image, size, fs, table, names):
    import json
    by_onyx = fs in [f for f, *_ in ONYX]
    if by_onyx:
        format_image(onyx, image, size, fs, table)
    vhd = as_vhd(image)
    number = mount_vhd(vhd)
    try:
        if not by_onyx:
            # Windows formats NTFS on a disk and not in a file, so onyx
            # formats the mounted VHD, which it finds by its number.
            drives = json.loads(run([onyx, "list", "--json"]))["drives"]
            drive = next((d for d in drives if d["id"] == str(number)), None)
            if drive is None:
                raise RuntimeError(f"onyx list does not show disk {number}")
            model = (drive.get("model") or "").strip() or drive["name"]
            run([onyx, "format", str(drive["number"]), "--fs", fs, "--table", table,
                 "--label", label_of(fs), "--force",
                 "--confirm", f"{model} {drive['size_bytes']}"])
        out = powershell(POWERSHELL_CHECK, number, TEXT)
        result = json.loads(out.strip().splitlines()[-1])
        return result["fs"] == names[2], result["fs"], result["label"], result["back"]
    finally:
        powershell("Dismount-DiskImage -ImagePath $args[0] | Out-Null", vhd)
        os.remove(vhd)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: check-format.py <path to onyx>")
    onyx = os.path.abspath(sys.argv[1])
    system = platform.system()
    if system not in HOST_TOOLS:
        sys.exit(f"this script does not know {system}")
    cases = [(fs, mib, table, names)
             for fs, mib, tables, *names in ONYX + HOST_TOOLS[system]
             for table in tables]
    directory = tempfile.mkdtemp(prefix="onyx-format-")
    failures = []
    try:
        for fs, mib, table, names in cases:
            what = f"{table.upper()} {fs}"
            image, size = make_image(directory, fs, mib, table)
            try:
                if system == "Windows":
                    right_kind, kind, label, back = check_windows(onyx, image, size, fs, table, names)
                else:
                    format_image(onyx, image, size, fs, table)
                    check = check_linux if system == "Linux" else check_macos
                    right_kind, kind, label, back = check(image, fs, names)
            except RuntimeError as e:
                failures.append(f"{what}: {e}")
                print(f"{what}: FAIL")
                continue
            finally:
                if os.path.exists(image):
                    os.remove(image)
            fails = []
            if not right_kind:
                fails.append(f"the host reads {kind!r}")
            if label.upper() != label_of(fs):
                fails.append(f"the label is {label!r}")
            if back != TEXT:
                fails.append(f"the file read back as {back!r}")
            print(f"{what}: checked, mounted, written, and read as {kind}, "
                  f"labelled {label}" + (", FAIL" if fails else ""))
            failures += [f"{what}: {f}" for f in fails]
    finally:
        shutil.rmtree(directory, ignore_errors=True)
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print(f"{len(cases)} layouts: the host checks, mounts, writes, and reads each one.")


if __name__ == "__main__":
    main()
