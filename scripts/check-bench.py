"""Compare `onyx bench --file` with fio, job by job.

This is the exit test of the benchmarks. It runs on Linux in CI, and a
person can run it wherever fio is:

    python3 scripts/check-bench.py target/release/onyx <directory>

Both tools use one test file of 1 GiB in the directory. A short run of Onyx
makes it and keeps it. fio then runs the jobs on it, then Onyx with
`--reuse`, then fio again, and the script deletes the file. A file that another program writes is laid out
in another way, and fio read such a file 20 percent slower than its own on an
APFS drive, so two files would compare two layouts and not two tools.

With a queue of one, fio sends one blocking `pread` or `pwrite` at a time
with its psync engine, as Onyx does. The asynchronous engines are not the same
job: on some Linux runners, libaio wrote 4 KiB at random 15 percent slower
than psync on the same file, and Onyx matched psync there on each of ten
runners. With a deeper queue, fio keeps `queue` commands in flight from one
thread, as CrystalDiskMark defines a queue, with no cache of the host: libaio
on Linux, windowsaio on Windows, and posixaio on macOS. A queue of threads is not the same job: each
fio thread reads or writes its own sequential stream, and Onyx shares one
stream between its threads. On one APFS drive, eight fio threads read at
2.8 GB/s and wrote at 11 GB/s, faster than the drive can write.

macOS runs posixaio on four threads of its own (`kern.aiothreads`), so a
queue deeper than four waits in fio and not in the drive. On one APFS drive,
fio read at 7.0 GB/s with a queue of eight from its own file, and at 1.2 to
2.2 GB/s from the file of Onyx, while fio with a queue of one and Onyx agreed
on that file. So on macOS only the jobs with a queue of one are compared. They take psync,
as on each host. Linux compares all four.

fio runs before Onyx and after it. A job passes when the speed of Onyx is
within 10 percent of the range that the two runs of fio span: at least 90
percent of the slower run, and at most 110 percent of the faster one. A disk
whose speed drifts during the check then moves the range around Onyx. When
Onyx ran first and fio twice after it, a cloud disk that slowed down gave
Onyx the fast part each time. The script prints all three
speeds, so a person sees how far apart they are. On the disk of a cloud
runner, two runs of fio differed by up to 28 percent, so CI runs this as a
report and the exit test runs on a quiet local drive.
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


# The threads that macOS runs posixaio on (`kern.aiothreads`). A deeper queue
# waits in fio and not in the drive.
MACOS_AIO_THREADS = 4


def fio(path, rw, bs, queue):
    """The speed of fio, in bytes per second, with `queue` commands in flight,
    or None when fio cannot run the job on this host."""
    system = platform.system()
    if queue == 1:
        options = ["--ioengine=psync"]
    elif system == "Linux":
        options = ["--ioengine=libaio", f"--iodepth={queue}"]
    elif system == "Windows":
        options = ["--ioengine=windowsaio", f"--iodepth={queue}"]
    elif queue <= MACOS_AIO_THREADS:
        options = ["--ioengine=posixaio", f"--iodepth={queue}"]
    else:
        return None
    command = [
        "fio", "--name=onyx", f"--filename={path}", f"--size={SIZE}",
        f"--rw={rw}", f"--bs={bs}", "--numjobs=1", *options, "--direct=1",
        f"--runtime={SECONDS}", "--time_based", "--output-format=json",
    ]
    done = subprocess.run(command, capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"fio failed: {done.stderr.strip()}")
    side = "read" if "read" in rw else "write"
    return json.loads(done.stdout)["jobs"][0][side]["bw_bytes"]


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: check-bench.py <path to onyx> <directory>")
    onyx, directory = sys.argv[1], sys.argv[2]
    test_file = os.path.join(directory, "onyx-bench.tmp")
    if os.path.exists(test_file):
        sys.exit(f"{test_file} exists. Remove it, or name another directory.")

    # Each read job first, then each write job, in the order of
    # CrystalDiskMark and of Onyx.
    def fio_pass():
        speeds = {}
        for name, read, _, bs, queue in JOBS:
            speeds[(name, False)] = fio(test_file, read, bs, queue)
        for name, _, write, bs, queue in JOBS:
            speeds[(name, True)] = fio(test_file, write, bs, queue)
        return speeds

    def onyx_pass(*extra):
        done = subprocess.run(
            [onyx, "bench", "--file", test_file, "--keep", "--json", *extra],
            check=True, capture_output=True, text=True,
        )
        return {(j["job"], j["write"]): j["bytes_per_second"] for j in json.loads(done.stdout)["jobs"]}

    try:
        # Lay out the file, so that fio measures the file that Onyx measures.
        onyx_pass("--seconds", "1")
        before = fio_pass()
        ours = onyx_pass("--reuse", "--seconds", str(SECONDS))
        after = fio_pass()
    finally:
        if os.path.exists(test_file):
            os.remove(test_file)

    failures = []
    skipped = 0
    print(f"{'JOB':<12} {'SIDE':<5} {'ONYX':>8} {'FIO 1':>8} {'FIO 2':>8}  MB/s")
    for name, *_ in JOBS:
        for write in (False, True):
            key = (name, write)
            side = "write" if write else "read"
            if before[key] is None or after[key] is None:
                skipped += 1
                print(f"{name:<12} {side:<5} {ours[key] / 1e6:>8.1f}  fio cannot run this job here")
                continue
            low = min(before[key], after[key]) * (1 - LIMIT)
            high = max(before[key], after[key]) * (1 + LIMIT)
            mark = "" if low <= ours[key] <= high else "  outside"
            print(f"{name:<12} {side:<5} {ours[key] / 1e6:>8.1f} {before[key] / 1e6:>8.1f} "
                  f"{after[key] / 1e6:>8.1f}{mark}")
            if mark:
                failures.append(f"{name} {side}: {ours[key] / 1e6:.1f} MB/s is not between "
                                f"{low / 1e6:.1f} and {high / 1e6:.1f}")
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        sys.exit(1)
    print(f"Each speed is within 10 percent of the range of fio. {skipped} were not compared.")


if __name__ == "__main__":
    main()
