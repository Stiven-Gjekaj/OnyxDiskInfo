<div align="center">
  <a href="README.md"><b>OnyxDiskInfo</b></a>
</div>

# Security Policy

## Supported versions

OnyxDiskInfo has no release yet.
When it has one, fixes go to the latest version on the default branch.
Older versions are not maintained.

## Reporting a vulnerability

Report security problems privately, and not through a public issue.

- Preferred: open a private security advisory with the "Report a vulnerability"
  button on the Security tab of this repository.
- Alternative: email the maintainer at stivenagostingjekaj@gmail.com.

Include the steps to reproduce, the affected commit, and the impact as you
understand it.
You can expect a first answer within a few days.
Your report gets an acknowledgement when the fix ships, unless you prefer to
stay anonymous.

## The threat model

**The Software runs with high privilege and opens raw devices.**
A pass-through command to a disk needs administrator rights on Windows, and
root rights on macOS and on Linux.
That is the job, and it is not a defect.
The question that matters is what the Software does with those rights.

**The worst outcome is a write to the wrong device.**
A format, an erase, or a write test on a disk that the person did not choose
destroys it.
This is the highest severity problem that this project can have.

**A system disk is never a target for a write.**
The Software refuses to write to the disk that the running system starts from.
A way around that refusal is a vulnerability, not a feature request.

**A command that reads never writes.**
`list`, `health`, the surface scan, and the benchmarks that only read must not
change a byte on the disk.

**The input is untrusted.**
A drive, a USB bridge, or a firmware returns bytes that an attacker can choose.
The output of `smartctl` is input too.

**These are in scope:**

- Any path that writes to a device that the person did not select and confirm.
- Any way to defeat the refusal to write to a system disk.
- A command that reads, but changes the disk.
- A decoder that crafted bytes from a device can crash, hang, or make read or
  write outside its buffer.
- A way to make the Software run a program other than the `smartctl` or the
  host format tool that it meant to run.
- A privilege escalation, such as a helper that a normal user can drive to do
  something the user cannot do alone.
- A verification pass that reports success for an erase or a write that did
  not happen.

**These are out of scope:**

- A report that the Software needs administrator or root rights.
- A report that the Software erases a drive when the person asks it to. That
  is what it does, and [TERMS.md](TERMS.md) section 4 says so.
- A wrong health value from a drive with a buggy firmware. That is a bug, so
  open a normal issue.
- Findings from an automated scanner with no working demonstration.
