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

fio runs twice, once before Onyx and once after it. A drive in a cloud
changes its speed from minute to minute, and two runs of fio on the same
runner differed by 7 percent. So a job passes when the speed of Onyx is
within 10 percent of the range that the two runs of fio span: at least 90
percent of the slower run, and at most 110 percent of the faster one. The
script prints all three speeds, so a person sees how far apart they are.
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

    def fio_pass():
        speeds = {}
        try:
            for name, read, write, bs, queue in JOBS:
                speeds[(name, False)] = fio(fio_file, read, bs, queue)
                speeds[(name, True)] = fio(fio_file, write, bs, queue)
        finally:
            if os.path.exists(fio_file):
                os.remove(fio_file)
        return speeds

    before = fio_pass()
    done = subprocess.run(
        [onyx, "bench", "--file", onyx_file, "--seconds", str(SECONDS), "--json"],
        check=True, capture_output=True, text=True,
    )
    ours = {(j["job"], j["write"]): j["bytes_per_second"] for j in json.loads(done.stdout)["jobs"]}
    after = fio_pass()

    failures = []
    print(f"{'JOB':<12} {'SIDE':<5} {'FIO 1':>8} {'ONYX':>8} {'FIO 2':>8}  MB/s")
    for name, *_ in JOBS:
        for write in (False, True):
            key = (name, write)
            low = min(before[key], after[key]) * (1 - LIMIT)
            high = max(before[key], after[key]) * (1 + LIMIT)
            side = "write" if write else "read"
            mark = "" if low <= ours[key] <= high else "  outside"
            print(f"{name:<12} {side:<5} {before[key] / 1e6:>8.1f} {ours[key] / 1e6:>8.1f} "
                  f"{after[key] / 1e6:>8.1f}{mark}")
            if mark:
                failures.append(f"{name} {side}: {ours[key] / 1e6:.1f} MB/s is not between "
                                f"{low / 1e6:.1f} and {high / 1e6:.1f}")
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print("Each speed is within 10 percent of the range of fio.")


if __name__ == "__main__":
    main()
