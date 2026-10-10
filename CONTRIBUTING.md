<div align="center">
  <a href="README.md"><b>OnyxDiskInfo</b></a>
</div>

# Contributing to OnyxDiskInfo

Thanks for your interest in OnyxDiskInfo.
It reads the health of every disk, and it tests, formats and erases a disk.
It runs on Windows, on macOS, and on Linux.
Contributions of all kinds are welcome: bug reports, dumps from real drives,
documentation fixes, support for a new bridge, and arguments against a
decision that is already made.

The toolchain is Rust, and it needs nothing else.

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo run -p onyx -- --version

Those four are the gate, and they run on Windows, on macOS and on Linux.
The [README](README.md) says what is built so far.

## Ways to contribute

- Send the raw SMART or NVMe data of a drive that Onyx decodes badly.
- Report a USB case, a card reader, or a RAID controller that Onyx cannot
  read.
- Add a bridge, a vendor attribute table, or a file system.
- Improve the documentation.
- Take on a phase from [docs/roadmap.md](docs/roadmap.md), or argue that the
  order is wrong, with the reason.

Before you start significant work, open an issue to agree on the approach.
This costs you one message and can save you a rewritten pull request.

## The rule that comes before all the others

**A test never opens a real device.**

Onyx formats and erases block devices.
A test suite that touches one destroys the disk of the person who runs it.
A continuous integration machine has a system disk, and that disk is a block
device.

Write the test against bytes or against a file.
A decoder takes a byte buffer, so a test gives it the bytes of a real drive
that somebody captured once.
A formatter takes a handle that reads, writes and seeks, so a test gives it a
file and reads the bytes back.

A pull request that adds a test which opens a path under `/dev`, a
`\\.\PhysicalDrive` path, or any real disk is refused.
This holds even when the test passes on your machine.

## How a host backend gets a test

Each host is two parts.

    onyx-core  decides everything and makes no system call.
               It is compiled on ALL THREE hosts, so its tests run three
               times on every change. Everything that can be wrong is here.

    the shim   makes system calls and decides nothing. It sends a command,
               returns the raw bytes, and hands them on. It is gated to its
               own host, and no test reaches it.

**A pure module must never name a type from `windows-sys`, `libc` or
`io-kit-sys`.**
Those crates belong to one host.
One such name removes the tests from the other two hosts, and nobody sees it.

A test builds the state it needs inside itself.
It never reads the devices of the computer that runs the test, because that
computer changes.

## Three more traps that this project creates

**A drive can lie.**
A firmware reports a value with a meaning that its maker chose.
A decoder that trusts one vendor table for all drives shows a wrong value with
confidence.
Put each vendor rule in the quirk table, and name the drive that needs it.

**Assert the property, not the byte sequence.**
Check that a partition table parses and that a file is at the offset you claim.
Do not compare a whole image against a stored copy.

**A size is not a state.**
A write is not complete because the counter reached the size of the source.
The bytes are in a cache until the operating system flushes them.

## What the code must never do

- Write to a device that the person did not select and confirm.
- Write to the disk that the running system starts from. **No flag allows
  this.**
- Write from a command that only reads, such as `list` or `health`.
- Show a health value that Onyx calculated as if the drive reported it.
- Report a test or a verification pass that it did not run.

## Coding style

- Match the surrounding code. Small, focused functions and clear names beat
  cleverness.
- Keep the code that writes a disk free of anything that talks to the person.
  It takes a target handle and a progress callback.
- Add dependencies sparingly, and say in the pull request why the standard
  library or an existing dependency does not do the job.
- Prefer MIT and Apache 2.0 for a dependency. Onyx links no GPL code.

## Commit messages and pull requests

[`AGENTS.md`](AGENTS.md) is the full set of rules. These four cause the most
rework.

- **One change per commit, and a feature is many commits.**
- **Code and its tests go in one commit. Documentation goes in its own.**
- **All text uses Simplified Technical English.** Short sentences, active voice,
  present tense. No em-dashes and no emoji.
- **Write the subject in the present tense, with no version number.**

Turn on the hook that checks the format before each commit:

    git config core.hooksPath .githooks

In the pull request, say what changed, why, and how you tested it.
Name the host and the drive.

## Reporting security issues

Do not open a public issue for a security problem.
See [SECURITY.md](SECURITY.md) for how to report it privately.

## Code of conduct

By taking part in this project you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
