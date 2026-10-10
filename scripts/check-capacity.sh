#!/bin/sh
# The exit test of the capacity test, on Linux, as root.
#
# It builds a fake drive the way fake flash is built: a device of 256 MiB
# whose four quarters all map onto the same 64 MiB. Onyx must call it fake and
# bracket the real end. A loop device of 64 MiB is the genuine control, and
# Onyx must call it genuine.
#
#     sudo scripts/check-capacity.sh target/debug/onyx
#
# It writes only to the two files that it makes, through the two devices
# that it makes, and removes all four at the end.
set -eu
onyx=$1
work=$(mktemp -d)
real="$work/real.img"
control="$work/control.img"
truncate -s 64M "$real" "$control"
loop=$(losetup --find --show "$real")
loop_control=$(losetup --find --show "$control")
cleanup() {
    dmsetup remove onyxfake 2>/dev/null || true
    losetup -d "$loop" "$loop_control" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

quarter=131072 # sectors of 512 bytes in 64 MiB
printf '%s\n' \
    "0 $quarter linear $loop 0" \
    "$quarter $quarter linear $loop 0" \
    "$((2 * quarter)) $quarter linear $loop 0" \
    "$((3 * quarter)) $quarter linear $loop 0" |
    dmsetup create onyxfake

status=0
"$onyx" test capacity --image /dev/mapper/onyxfake \
    --confirm "onyxfake 268435456" --json >"$work/fake.json" || status=$?
"$onyx" test capacity --image "$loop_control" \
    --confirm "$(basename "$loop_control") 67108864" --json >"$work/control.json"

python3 - "$work/fake.json" "$work/control.json" "$status" <<'PY'
import json, sys
fake = json.load(open(sys.argv[1]))
control = json.load(open(sys.argv[2]))
status = int(sys.argv[3])
real = 64 * 1024 * 1024
probe = 64 * 1024
failures = []
if fake["genuine"]:
    failures.append("the fake device passed as genuine")
if fake["first_fault"] != "wraps":
    failures.append(f"the first fault is {fake['first_fault']}, not wraps")
detail = fake["first_fault_detail"] or {}
if fake["first_fault"] == "wraps" and detail["from"] - detail["onto"] != real:
    failures.append(f"the drive wraps every {detail['from'] - detail['onto']}, not {real}")
if not fake["proven_bytes"] <= real < fake["first_fault_offset"] + probe:
    failures.append(f"the real end {real} is not between {fake['proven_bytes']} "
                    f"and {fake['first_fault_offset'] + probe}")
if status != 2:
    failures.append(f"the exit code for a fake drive is {status}, not 2")
if not fake["restored"]:
    failures.append("the saved blocks of the fake device did not go back")
if not control["genuine"]:
    failures.append("the genuine loop device failed")
print(f"fake: genuine={fake['genuine']}, first fault {fake['first_fault']} at "
      f"{fake['first_fault_offset']}, proven {fake['proven_bytes']} of {fake['claimed_bytes']}")
print(f"control: genuine={control['genuine']}, {control['faults']} faults")
for f in failures:
    print(f"FAIL {f}")
sys.exit(1 if failures else 0)
PY
