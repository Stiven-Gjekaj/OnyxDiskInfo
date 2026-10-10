"""Format images with `onyx format`, and let the host check and mount each one.

This is the exit test of P6 for the file systems that Onyx writes itself. For
each table, GPT and MBR, and each file system, FAT12, FAT16, FAT32, and
exFAT, the script:

1. makes an empty image and formats it with `onyx format --image`,
2. attaches it as a disk with the tool of the host,
3. checks the partition with the check tool of the host,
4. mounts it, writes a file, reads the file back, and
5. compares the file system and the label that the host reports.

    sudo python3 scripts/check-format.py target/debug/onyx        # Linux
    python3 scripts/check-format.py target/debug/onyx             # macOS
    python scripts/check-format.py target/debug/onyx.exe          # Windows

Linux attaches with losetup and checks with fsck.fat and fsck.exfat, so it
needs root, dosfstools, and exfatprogs. macOS attaches with hdiutil and checks
with fsck_msdos and fsck_exfat. Windows mounts a VHD, which is the image with
a footer of 512 bytes after it, and checks with chkdsk. Windows needs an
Administrator.

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

# Each file system, the size of its image, and the name that each host
# gives it.
FILE_SYSTEMS = [
    # name, MiB, Linux blkid TYPE and VERSION, macOS personality, Windows
    ("fat12", 16, ("vfat", "FAT12"), "MS-DOS FAT12", "FAT"),
    ("fat16", 128, ("vfat", "FAT16"), "MS-DOS FAT16", "FAT"),
    ("fat32", 600, ("vfat", "FAT32"), "MS-DOS FAT32", "FAT32"),
    ("exfat", 64, ("exfat", None), "ExFAT", "exFAT"),
]
TABLES = ["mbr", "gpt"]
TEXT = "Onyx wrote this volume, and the host reads it back.\n"


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


def make_image(onyx, directory, fs, mib, table):
    """An image of `mib` MiB with a layout that onyx writes. Gives its path."""
    name = f"{fs}-{table}.img"
    path = os.path.join(directory, name)
    size = mib * MIB
    with open(path, "wb") as f:
        f.truncate(size)
    run([onyx, "format", "--image", path, "--fs", fs, "--table", table,
         "--label", f"ONYX {fs.upper()}", "--confirm", f"{name} {size}"])
    return path


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
        for _ in range(50):
            if os.path.exists(part):
                break
            time.sleep(0.1)
        tool = "fsck.exfat" if fs == "exfat" else "fsck.fat"
        run([tool, "-n", part])
        kind = run(["blkid", "-o", "value", "-s", "TYPE", part]).strip()
        version = run(["blkid", "-o", "value", "-s", "VERSION", part]).strip() or None
        label = run(["blkid", "-o", "value", "-s", "LABEL", part]).strip()
        run(["mount", part, mount])
        try:
            back = write_and_read(mount)
        finally:
            run(["umount", mount])
        run([tool, "-n", part])
        want_kind, want_version = names[0]
        shown = (kind, version if want_version else None)
        return shown == (want_kind, want_version), f"{kind} {version or ''}".strip(), label, back
    finally:
        os.rmdir(mount)
        run(["losetup", "--detach", device])


def check_macos(image, fs, names):
    attached = run(["hdiutil", "attach", "-imagekey", "diskimage-class=CRawDiskImage",
                    "-nomount", image])
    disk = attached.split()[0]
    part = f"{disk}s1"
    mount = tempfile.mkdtemp()
    try:
        tool = "fsck_exfat" if fs == "exfat" else "fsck_msdos"
        run([tool, "-n", part])
        info = run(["diskutil", "info", part])
        fields = dict(line.strip().split(":", 1) for line in info.splitlines() if ":" in line)
        kind = fields.get("File System Personality", "").strip()
        label = fields.get("Volume Name", "").strip()
        run(["mount", "-t", "exfat" if fs == "exfat" else "msdos", part, mount])
        try:
            back = write_and_read(mount)
        finally:
            run(["umount", mount])
        run([tool, "-n", part])
        return kind == names[1], kind, label, back
    finally:
        os.rmdir(mount)
        run(["hdiutil", "detach", "-quiet", disk])


POWERSHELL_CHECK = r"""
$ErrorActionPreference = 'Stop'
$vhd = $args[0]
$disk = Mount-DiskImage -ImagePath $vhd -Access ReadWrite -PassThru | Get-Disk
try {
    $part = Get-Partition -DiskNumber $disk.Number -PartitionNumber 1
    if (-not $part.DriveLetter) {
        $part | Add-PartitionAccessPath -AssignDriveLetter
        $part = Get-Partition -DiskNumber $disk.Number -PartitionNumber 1
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
}
finally {
    Dismount-DiskImage -ImagePath $vhd | Out-Null
}
"""


def check_windows(image, fs, names, directory):
    import json
    size = os.path.getsize(image)
    vhd = image[:-4] + ".vhd"
    os.replace(image, vhd)
    with open(vhd, "ab") as f:
        f.write(vhd_footer(size))
    script = os.path.join(directory, "check.ps1")
    with open(script, "w") as f:
        f.write(POWERSHELL_CHECK)
    try:
        out = run(["pwsh", "-NoProfile", "-File", script, os.path.abspath(vhd), TEXT])
        result = json.loads(out.strip().splitlines()[-1])
        return result["fs"] == names[2], result["fs"], result["label"], result["back"]
    finally:
        os.remove(vhd)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: check-format.py <path to onyx>")
    onyx = os.path.abspath(sys.argv[1])
    system = platform.system()
    directory = tempfile.mkdtemp(prefix="onyx-format-")
    failures = []
    try:
        for table in TABLES:
            for fs, mib, *names in FILE_SYSTEMS:
                what = f"{table.upper()} {fs}"
                try:
                    image = make_image(onyx, directory, fs, mib, table)
                    if system == "Linux":
                        right_kind, kind, label, back = check_linux(image, fs, names)
                    elif system == "Darwin":
                        right_kind, kind, label, back = check_macos(image, fs, names)
                    elif system == "Windows":
                        right_kind, kind, label, back = check_windows(image, fs, names, directory)
                    else:
                        sys.exit(f"this script does not know {system}")
                except RuntimeError as e:
                    failures.append(f"{what}: {e}")
                    print(f"{what}: FAIL")
                    continue
                finally:
                    if os.path.exists(os.path.join(directory, f"{fs}-{table}.img")):
                        os.remove(os.path.join(directory, f"{fs}-{table}.img"))
                fails = []
                if not right_kind:
                    fails.append(f"the host reads {kind!r}")
                if label.upper() != f"ONYX {fs.upper()}":
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
    count = len(TABLES) * len(FILE_SYSTEMS)
    print(f"{count} layouts: the host checks, mounts, writes, and reads each one.")


if __name__ == "__main__":
    main()
