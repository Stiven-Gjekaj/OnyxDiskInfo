"""Compare `onyx bench --file` with fio, job by job.

This is the exit test of the benchmarks. It runs on Linux in CI, and a
person can run it wherever fio is:

    python3 scripts/check-bench.py target/release/onyx <directory>

Each tool gets its own test file of 1 GiB in the directory, and each one
deletes it. fio runs each job as CrystalDiskMark defines it: one thread with
`queue` commands in flight, through the asynchronous engine of the host, and
no cache of the host. Onyx keeps the same number of commands in flight with
one thread for each. The script passes when each speed of Onyx is within 10
percent of the speed of fio.

The first version of this script gave fio `queue` threads of its own. Each
fio thread then read or wrote its own sequential stream, and Onyx shares one
stream between its threads, so the two did not run the same job.

The two tools run one after the other on a drive that other programs use as
well, so two runs of the same tool also differ. The script prints each pair,
so a person sees how far apart they are.
"""

import json
import os
import platform
import subprocess
import sys

SECONDS = 5
SIZE = "1g"
LIMIT = 0.10

# The jobs of CrystalDiskMark, as fio takes them.
JOBS = [
    ("SEQ1M Q8T1", "read", "write", "1m", 8),
    ("SEQ1M Q1T1", "read", "write", "1m", 1),
    ("RND4K Q32T1", "randread", "randwrite", "4k", 32),
    ("RND4K Q1T1", "randread", "randwrite", "4k", 1),
]


# The asynchronous engine of each host, which keeps a queue of commands in
# flight from one thread.
ENGINE = {"Linux": "libaio", "Darwin": "posixaio", "Windows": "windowsaio"}


def fio(path, rw, bs, queue):
    command = [
        "fio", "--name=onyx", f"--filename={path}", f"--size={SIZE}",
        f"--rw={rw}", f"--bs={bs}", "--numjobs=1", f"--iodepth={queue}",
        f"--ioengine={ENGINE[platform.system()]}", "--direct=1",
        f"--runtime={SECONDS}", "--time_based", "--output-format=json",
    ]
    out = json.loads(subprocess.run(command, check=True, capture_output=True, text=True).stdout)
    job = out["jobs"][0]
    side = "read" if "read" in rw else "write"
    return job[side]["bw_bytes"]


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: check-bench.py <path to onyx> <directory>")
    onyx, directory = sys.argv[1], sys.argv[2]
    fio_file = os.path.join(directory, "fio-bench.tmp")
    onyx_file = os.path.join(directory, "onyx-bench.tmp")

    theirs = {}
    try:
        for name, read, write, bs, queue in JOBS:
            theirs[(name, False)] = fio(fio_file, read, bs, queue)
            theirs[(name, True)] = fio(fio_file, write, bs, queue)
    finally:
        if os.path.exists(fio_file):
            os.remove(fio_file)

    done = subprocess.run(
        [onyx, "bench", "--file", onyx_file, "--seconds", str(SECONDS), "--json"],
        check=True, capture_output=True, text=True,
    )
    ours = {(j["job"], j["write"]): j["bytes_per_second"] for j in json.loads(done.stdout)["jobs"]}

    failures = []
    print(f"{'JOB':<12} {'SIDE':<5} {'ONYX MB/s':>10} {'FIO MB/s':>10} {'DIFF':>7}")
    for name, *_ in JOBS:
        for write in (False, True):
            a, b = ours[(name, write)], theirs[(name, write)]
            diff = (a - b) / b if b else float("inf")
            side = "write" if write else "read"
            print(f"{name:<12} {side:<5} {a / 1e6:>10.1f} {b / 1e6:>10.1f} {diff:>+7.1%}")
            if abs(diff) > LIMIT:
                failures.append(f"{name} {side}: {diff:+.1%}")
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print("Each speed is within 10 percent of fio.")


if __name__ == "__main__":
    main()
