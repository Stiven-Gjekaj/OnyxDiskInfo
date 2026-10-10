"""Compare `onyx list --json` with the tool of the host.

This is the exit test of P1. It runs on each host in CI, and a person can run
it on a real machine:

    python3 scripts/check-list.py target/debug/onyx

The host tools are `lsblk` on Linux, `diskutil` on macOS, and `Get-Disk` on
Windows. The script compares each drive that both tools report:

- The set of drives must be the same.
- The size must be the same, to the byte.
- The serial number and the firmware must be the same, where the host tool
  gives one.

A value that only Onyx gives is not a failure, and the script says so.
It reads every drive and writes nothing.
"""

import json
import platform
import plistlib
import subprocess
import sys

# The kernel names that Onyx does not list on Linux. This is the same list as
# NOT_A_DRIVE in crates/onyx-probe/src/linux/parse.rs.
LINUX_NOT_A_DRIVE = ("loop", "ram", "zram", "sr", "dm-", "md", "nbd", "fd")


def run(command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def clean(value):
    """A value as text, with the padding off, or None when it is empty."""
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def linux_drives():
    out = json.loads(run(["lsblk", "-J", "-b", "-d", "-o", "NAME,TYPE,SIZE,SERIAL,REV"]))
    drives = {}
    for d in out["blockdevices"]:
        if d["type"] != "disk" or d["name"].startswith(LINUX_NOT_A_DRIVE):
            continue
        if int(d["size"]) == 0:
            continue
        drives[d["name"]] = {
            "size_bytes": int(d["size"]),
            "serial": clean(d.get("serial")),
            "firmware": clean(d.get("rev")),
        }
    return drives


def macos_drives():
    listing = plistlib.loads(run(["diskutil", "list", "-plist", "physical"]).encode())
    drives = {}
    for name in listing["WholeDisks"]:
        info = plistlib.loads(run(["diskutil", "info", "-plist", name]).encode())
        if info.get("TotalSize", 0) == 0:
            continue
        # diskutil gives no serial and no firmware. system_profiler does, and
        # a person checks those by hand.
        drives[name] = {"size_bytes": info["TotalSize"], "serial": None, "firmware": None}
    return drives


def windows_drives():
    script = (
        "Get-Disk | Select-Object Number, Size, SerialNumber, FirmwareVersion"
        " | ConvertTo-Json -AsArray"
    )
    out = json.loads(run(["powershell", "-NoProfile", "-Command", script]))
    drives = {}
    for d in out:
        if int(d["Size"]) == 0:
            continue
        drives[str(d["Number"])] = {
            "size_bytes": int(d["Size"]),
            "serial": clean(d.get("SerialNumber")),
            "firmware": clean(d.get("FirmwareVersion")),
        }
    return drives


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: check-list.py <path to onyx>")
    onyx = json.loads(run([sys.argv[1], "list", "--json"]))
    ours = {d["id"]: d for d in onyx["drives"]}

    host = {
        "Linux": linux_drives,
        "Darwin": macos_drives,
        "Windows": windows_drives,
    }[platform.system()]()

    failures = []
    if set(ours) != set(host):
        failures.append(f"the drives differ: onyx {sorted(ours)}, host {sorted(host)}")

    for name in sorted(set(ours) & set(host)):
        mine, theirs = ours[name], host[name]
        if mine["size_bytes"] != theirs["size_bytes"]:
            failures.append(
                f"{name}: size {mine['size_bytes']} and not {theirs['size_bytes']}"
            )
        for field in ("serial", "firmware"):
            if theirs[field] is None:
                if mine[field] is not None:
                    print(f"{name}: only onyx gives the {field}, {mine[field]!r}")
                continue
            if clean(mine[field]) != theirs[field]:
                failures.append(f"{name}: {field} {mine[field]!r} and not {theirs[field]!r}")
        print(f"{name}: {mine['name']}, {mine['size_bytes']} bytes, checked")

    if failures:
        for failure in failures:
            print(f"FAIL {failure}")
        sys.exit(1)
    print(f"{len(ours)} drives agree with the host tool.")


if __name__ == "__main__":
    main()
