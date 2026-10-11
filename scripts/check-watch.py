"""The exit test of P7: a value that changes between two polls makes an alert.

The drives of a runner do not change in a minute, so the script makes the
change in the history. It polls once with `onyx watch --once`, lowers one
count of damage of the last reading in the history file, and polls again.
The new reading of the real drive then counts more than the reading before
it, and the whole path must say so: the read of the drive, the history,
the comparison, the exit code, and `onyx alerts`.

    sudo python3 scripts/check-watch.py target/debug/onyx       # Linux, macOS
    python scripts/check-watch.py target/debug/onyx.exe         # Windows

The script writes only to a history file that it makes in a new folder. It
reads the drives, and it writes nothing on a drive.
"""

import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile

# Counts of damage that `onyx watch` compares, in the order to try them.
DAMAGE = [
    "nvme.media_errors",
    "measured.failed_reads",
    "ata.5.raw",
    "ata.197.raw",
    "ata.199.raw",
    "ata.187.raw",
]


def watch(onyx, db):
    done = subprocess.run([onyx, "watch", "--once", "--db", db, "--json"],
                          capture_output=True, text=True)
    if done.returncode not in (0, 2):
        sys.exit(f"onyx watch gave {done.returncode}: {done.stderr.strip()}")
    return done.returncode, json.loads(done.stdout)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: check-watch.py <path to onyx>")
    onyx = os.path.abspath(sys.argv[1])
    folder = tempfile.mkdtemp(prefix="onyx-watch-")
    db = os.path.join(folder, "history.sqlite")
    try:
        code, first = watch(onyx, db)
        stored = [d for d in first["drives"] if d["stored"]]
        for d in first["drives"]:
            print(f"{d['id']}: {'kept' if d['stored'] else 'not read: ' + str(d['error'])}")
        if not stored:
            sys.exit("onyx watch kept no reading, so there is nothing to compare")
        if code != 0:
            sys.exit(f"the first poll gave exit code {code}, and a first poll has no alert")

        # Lower one count of damage of the last reading of one drive.
        con = sqlite3.connect(db)
        target = None
        for name in DAMAGE:
            row = con.execute(
                """SELECT r.id, d.key, v.value FROM value v
                   JOIN reading r ON r.id = v.reading JOIN drive d ON d.id = r.drive
                   WHERE v.name = ? ORDER BY r.id DESC LIMIT 1""", (name,)).fetchone()
            if row:
                target = (name, *row)
                break
        if target is None:
            sys.exit("no reading holds a count of damage that onyx compares")
        name, reading, key, value = target
        con.execute("UPDATE value SET value = ? WHERE reading = ? AND name = ?",
                    (value - 1, reading, name))
        con.commit()
        con.close()
        print(f"{key}: {name} in the history set from {value} to {value - 1}")

        code, second = watch(onyx, db)
        alerts = [a for d in second["drives"] if d["key"] == key for a in d["alerts"]]
        for a in alerts:
            print(f"alert: {a['severity']}: {a['message']}")
        failures = []
        if not any(a["value"] == name for a in alerts):
            failures.append(f"the second poll gave no alert for {name}")
        if code != 2:
            failures.append(f"the second poll gave exit code {code}, and an alert gives 2")
        listed = json.loads(subprocess.run(
            [onyx, "alerts", "--db", db, "--json"],
            check=True, capture_output=True, text=True).stdout)
        if not any(a["value"] == name and a["drive"] == key for a in listed):
            failures.append("onyx alerts does not show the alert")
    finally:
        shutil.rmtree(folder, ignore_errors=True)
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print("A value that changed between two polls made an alert.")


if __name__ == "__main__":
    main()
