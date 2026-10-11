"""Erase an image that the host attaches as a disk, and read the image back.

This is the check of `onyx erase` on a disk node of the host, and not on a
file. The script fills an image with the byte 0xA5 and attaches it as a
disk: with losetup on Linux, with hdiutil on macOS, and as a VHD on Windows.

1. `onyx erase --pass random --pass zero` must report no bad block, and the
   image must hold only zeros after it.
2. `onyx erase --trim` must run to the end. The script prints how many of
   the sample blocks read back as zeros. Only the loop device of Linux
   passes a TRIM down to the file, so only Linux must read all zeros. The
   disk images of macOS and Windows take the TRIM and keep the bytes, as a
   drive is free to do.

    sudo python3 scripts/check-erase.py target/debug/onyx        # Linux
    python3 scripts/check-erase.py target/debug/onyx             # macOS
    python scripts/check-erase.py target/debug/onyx.exe          # Windows

The script writes only to the image that it makes.
"""

import json
import os
import platform
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
check_format = __import__("check-format")

MIB = 1024 * 1024
SIZE = 64 * MIB


def run(command):
    return check_format.run(command)


def fill(path):
    with open(path, "wb") as f:
        f.write(b"\xa5" * SIZE)


def only_zeros(path):
    with open(path, "rb") as f:
        while block := f.read(MIB):
            if block.count(0) != len(block):
                return False
    return True


def attach(image):
    """Attach the image as a disk. Gives the arguments that name it to onyx,
    and a function that detaches it."""
    system = platform.system()
    if system == "Linux":
        device = run(["losetup", "--find", "--show", image]).strip()
        name = os.path.basename(device)
        # A block device that onyx list does not show goes in as an image.
        return ["--image", device], f"{name} {SIZE}", lambda: run(["losetup", "--detach", device])
    if system == "Darwin":
        out = run(["hdiutil", "attach", "-imagekey", "diskimage-class=CRawDiskImage",
                   "-nomount", image])
        disk = out.split()[0]
        target = drive_args(disk.removeprefix("/dev/"))
        return target[0], target[1], lambda: check_format.hdiutil_detach(disk)
    vhd = check_format.as_vhd(image)
    number = check_format.mount_vhd(vhd)
    target = drive_args(str(number))

    def detach():
        check_format.powershell("Dismount-DiskImage -ImagePath $args[0] | Out-Null", vhd)
        os.replace(vhd, image)
        with open(image, "r+b") as f:
            f.truncate(SIZE)
    return target[0], target[1], detach


def drive_args(drive_id):
    """The number of a drive in `onyx list`, and the phrase that confirms it."""
    drives = json.loads(run([ONYX, "list", "--json"]))["drives"]
    drive = next((d for d in drives if d["id"] == drive_id), None)
    if drive is None:
        raise RuntimeError(f"onyx list does not show {drive_id}")
    model = (drive.get("model") or "").strip() or drive["name"]
    return [str(drive["number"]), "--force"], f"{model} {drive['size_bytes']}"


def erase(image, extra):
    """Attach, erase with the extra arguments, detach. Gives the JSON."""
    target, phrase, detach = attach(image)
    try:
        out = run([ONYX, "erase", *target, *extra, "--confirm", phrase, "--json"])
    finally:
        detach()
    return json.loads(out.strip().splitlines()[-1])


def main():
    global ONYX
    if len(sys.argv) != 2:
        sys.exit("usage: check-erase.py <path to onyx>")
    ONYX = os.path.abspath(sys.argv[1])
    failures = []
    directory = tempfile.mkdtemp(prefix="onyx-erase-")
    image = os.path.join(directory, "erase.img")
    try:
        fill(image)
        result = erase(image, ["--pass", "random", "--pass", "zero"])
        zeros = only_zeros(image)
        print(f"overwrite: {result['bytes_written']} bytes written, "
              f"{result['bytes_checked']} read back, {result['bad_blocks']} bad blocks, "
              f"the image holds only zeros: {zeros}")
        if result["bad_blocks"] or not result["complete"] or not zeros:
            failures.append(f"overwrite: {result}")

        fill(image)
        result = erase(image, ["--trim"])
        print(f"TRIM: {result['zero_samples']} of {result['samples']} sample blocks read zeros")
        if platform.system() == "Linux" and result["zero_samples"] != result["samples"]:
            failures.append(f"TRIM of a loop device left old bytes: {result}")
    except RuntimeError as e:
        failures.append(str(e))
    finally:
        if os.path.exists(image):
            os.remove(image)
        os.rmdir(directory)
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print("onyx erase works on a disk of this host.")


if __name__ == "__main__":
    main()
