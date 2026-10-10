<div align="center">
  <a href="README.md"><b>OnyxDiskInfo</b></a>
</div>

# Getting help

OnyxDiskInfo is in the planning stage.
There is no release to install yet, so most help today is about the plan.

## Understand the project

- [README.md](README.md) says what the project is and what state it is in.
- [docs/roadmap.md](docs/roadmap.md) holds the order that the work happens
  in, and the test that ends each phase.
- [docs/milestones.md](docs/milestones.md) holds the decisions that shape the
  work, and the reason behind each one.
- [AGENTS.md](AGENTS.md) sets the rules for anybody who changes this
  repository, human or agent.
- [TERMS.md](TERMS.md) says what the Software does to your data, and what you
  agree to when you run it.

## Ask a question or report a problem

- Search the existing
  [issues](https://github.com/Stiven-Gjekaj/OnyxDiskInfo/issues) first, in
  case somebody has already asked.
- If you found a bug, open a bug report.
- If you want a feature, open a feature request.

When you report a wrong health value, name the drive and how it connects.
A drive in a USB case is a different report from the same drive on SATA.
Add the output of `smartctl -a` if you have it, because it shows what the
drive reports to a second tool.

Do not use the issue tracker for security problems.
See [SECURITY.md](SECURITY.md) for how to report those privately.

## Contributing

If you want to help build OnyxDiskInfo, see [CONTRIBUTING.md](CONTRIBUTING.md).
