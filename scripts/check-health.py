"""Compare `onyx health --json` with `smartctl --json -a` for each drive.

This is the exit test of P2. It runs on each host in CI, and a person can run
it on a real machine:

    python3 scripts/check-health.py target/debug/onyx

Linux needs root for both tools, so CI runs it with sudo there.

For each drive in `onyx list`:

- When neither tool reads health data, the two agree, and the script says so.
- When only smartctl reads it, the script fails.
- When both read it, the identity and each counter must agree. A value that
  moves between the two reads, such as the temperature or the hours, may
  differ by a little. The data counters are not compared, because a drive
  in use changes them every second.

The script sends read commands only. It writes nothing.
"""

import json
import platform
import shutil
import subprocess
import sys

# How far a value may move between the read of onyx and the read of smartctl.
TEMPERATURE_SLACK = 3
HOURS_SLACK = 1

# ATA attributes whose raw value moves while the drive runs.
ATA_MOVING = {9, 190, 194, 241, 242}


def smartctl_device(drive):
    """The name that smartctl takes for a drive from `onyx list --json`."""
    system = platform.system()
    if system == "Darwin":
        return drive["id"]
    if system == "Windows":
        return f"/dev/pd{drive['id']}"
    return drive["node"]


def onyx_health(onyx, number):
    done = subprocess.run(
        [onyx, "health", str(number), "--json"], capture_output=True, text=True
    )
    if done.returncode != 0:
        return None, done.stderr.strip()
    return json.loads(done.stdout), None


def smartctl_health(device):
    done = subprocess.run(
        ["smartctl", "--json", "-a", device], capture_output=True, text=True
    )
    # smartctl sets bits in its exit code for warnings as well as for
    # failures, so the JSON decides and not the code.
    try:
        return json.loads(done.stdout)
    except json.JSONDecodeError:
        return {}


def near(a, b, slack):
    return a is not None and b is not None and abs(a - b) <= slack


def compare_nvme(mine, theirs, fail):
    log = theirs["nvme_smart_health_information_log"]
    ours = mine["nvme"]
    for key, their_key in [
        ("critical_warning", "critical_warning"),
        ("available_spare", "available_spare"),
        ("available_spare_threshold", "available_spare_threshold"),
        ("percentage_used", "percentage_used"),
        ("unsafe_shutdowns", "unsafe_shutdowns"),
        ("media_errors", "media_errors"),
        ("error_log_entries", "num_err_log_entries"),
    ]:
        if ours[key] != log[their_key]:
            fail(f"{key} {ours[key]} and not {log[their_key]}")
    if mine["power_cycles"] != log["power_cycles"]:
        fail(f"power_cycles {mine['power_cycles']} and not {log['power_cycles']}")
    if not near(mine["power_on_hours"], log["power_on_hours"], HOURS_SLACK):
        fail(f"power_on_hours {mine['power_on_hours']} and not {log['power_on_hours']}")
    if not near(mine["temperature_celsius"], log["temperature"], TEMPERATURE_SLACK):
        fail(f"temperature {mine['temperature_celsius']} and not {log['temperature']}")


def compare_ata(mine, theirs, fail):
    table = {a["id"]: a for a in theirs["ata_smart_attributes"]["table"]}
    ours = {a["id"]: a for a in mine["attributes"]}
    if set(ours) != set(table):
        fail(f"attributes {sorted(ours)} and not {sorted(table)}")
    for id in sorted(set(ours) & set(table)):
        a, b = ours[id], table[id]
        if (a["current"], a["worst"]) != (b["value"], b["worst"]):
            fail(f"attribute {id}: {a['current']}/{a['worst']} and not {b['value']}/{b['worst']}")
        if a["threshold"] is not None and a["threshold"] != b["thresh"]:
            fail(f"attribute {id}: threshold {a['threshold']} and not {b['thresh']}")
        if id not in ATA_MOVING and a["raw"] != b["raw"]["value"]:
            fail(f"attribute {id}: raw {a['raw']} and not {b['raw']['value']}")


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: check-health.py <path to onyx>")
    if shutil.which("smartctl") is None:
        sys.exit("smartctl is not on the PATH. Install smartmontools first.")
    onyx = sys.argv[1]
    drives = json.loads(
        subprocess.run([onyx, "list", "--json"], check=True, capture_output=True, text=True).stdout
    )["drives"]

    failures = []
    compared = 0
    for drive in drives:
        label = f"{drive['number']} ({drive['id']}, {drive['name']})"
        mine, why = onyx_health(onyx, drive["number"])
        theirs = smartctl_health(smartctl_device(drive))
        smartctl_reads = (
            "nvme_smart_health_information_log" in theirs or "ata_smart_attributes" in theirs
        )

        if mine is None:
            if smartctl_reads:
                failures.append(f"{label}: smartctl reads health data and onyx does not: {why}")
            else:
                print(f"{label}: neither tool reads health data. onyx: {why}")
            continue
        if not smartctl_reads:
            print(f"{label}: only onyx reads health data, so nothing is compared")
            continue

        def fail(message, label=label):
            failures.append(f"{label}: {message}")

        for key, their_key in [
            ("model", "model_name"),
            ("serial", "serial_number"),
            ("firmware", "firmware_version"),
        ]:
            if mine[key] != theirs.get(their_key):
                fail(f"{key} {mine[key]!r} and not {theirs.get(their_key)!r}")
        if mine["protocol"] == "NVMe":
            compare_nvme(mine, theirs, fail)
        else:
            compare_ata(mine, theirs, fail)
        passed = theirs.get("smart_status", {}).get("passed")
        if passed is False and mine["health"]["verdict"] != "Bad":
            fail(f"smartctl says the drive fails, and onyx says {mine['health']['verdict']}")
        compared += 1
        print(f"{label}: {mine['protocol']}, {mine['health']['verdict']}, compared")

    if failures:
        for failure in failures:
            print(f"FAIL {failure}")
        sys.exit(1)
    print(f"{compared} drives compared, and each one agrees with smartctl.")


if __name__ == "__main__":
    main()
