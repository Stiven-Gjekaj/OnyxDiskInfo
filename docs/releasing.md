<div align="center">
  <a href="../README.md"><b>OnyxDiskInfo</b></a>
</div>

# Releasing

A release is a decision that a person makes. The workflow does the rest, and
it does nothing until a tag tells it to. Each package manager gets the release
from a person, and no bot pushes to a tap or a bucket.

## Before a release

Every item has to be true, and not only probably true.

- CI is green on the commit that the tag will name, on the three hosts. That
  includes the job that checks the package scripts.
- The version in the root `Cargo.toml` is the version of the release.
- [CHANGELOG.md](../CHANGELOG.md) has a section for the version, such as
  `## 0.1.0 - 2026-10-11`. The workflow makes the notes of the release from
  it, and it stops when there is none.

## Cut a release

1. Set `version` under `[workspace.package]` in the root `Cargo.toml`, and
   run `cargo check` so `Cargo.lock` follows. Commit both. The commit subject
   names no version number.
2. Push, and wait for CI to pass on that commit.
3. Tag the commit and push the tag:

       git tag v0.1.0
       git push origin v0.1.0

4. The [release workflow](../.github/workflows/release.yml) builds the files,
   hashes each one, attests each one, and opens a **draft** release. It
   refuses a tag that disagrees with the version in `Cargo.toml`, because that
   binary would name a release that it does not belong to.
5. Open the draft. Read the list of assets: the nine files in the table
   below and `SHA256SUMS`. Publish it.
6. Get the checksums of the release:

       gh release download v0.1.0 --pattern SHA256SUMS --dir release

Then give the release to each package manager. Each step writes the files from
`SHA256SUMS` with a script of this repository, so no person copies a digest by
hand.

### Homebrew

Write the formula of the command line and the cask of the app into the tap,
[Stiven-Gjekaj/homebrew-tap](https://github.com/Stiven-Gjekaj/homebrew-tap),
and commit them there:

    scripts/homebrew-formula.sh 0.1.0 release/SHA256SUMS > ../homebrew-tap/Formula/onyxdiskinfo.rb
    scripts/homebrew-cask.sh 0.1.0 release/SHA256SUMS > ../homebrew-tap/Casks/onyxdiskinfo.rb

The formula and the cask have the same name, so the app takes `--cask`:

    brew install stiven-gjekaj/tap/onyxdiskinfo
    brew install --cask stiven-gjekaj/tap/onyxdiskinfo

### Scoop

Write the manifest into the bucket,
[Stiven-Gjekaj/scoop-bucket](https://github.com/Stiven-Gjekaj/scoop-bucket),
and commit it there:

    scripts/scoop-manifest.sh 0.1.0 release/SHA256SUMS > ../scoop-bucket/bucket/onyxdiskinfo.json

### winget

Write the manifests into a branch of the fork
[Stiven-Gjekaj/winget-pkgs](https://github.com/Stiven-Gjekaj/winget-pkgs),
check them on Windows, and open a pull request to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) with them.
The repository holds hundreds of thousands of files, so sync the fork and work
in the sparse clone of the folder of the publisher:

    gh repo sync Stiven-Gjekaj/winget-pkgs
    git -C ../winget-pkgs checkout -b Stiven-Gjekaj.OnyxDiskInfo-0.1.0
    scripts/winget-manifest.sh 0.1.0 release/SHA256SUMS ../winget-pkgs
    winget validate --manifest ..\winget-pkgs\manifests\s\Stiven-Gjekaj\OnyxDiskInfo\0.1.0

The title of the pull request is `New package: Stiven-Gjekaj.OnyxDiskInfo
version 0.1.0` for the first version, and
`Update: Stiven-Gjekaj.OnyxDiskInfo to <version>` for each later one.
Microsoft checks the pull request, and a bot merges it.

## Check the install

The [install check](../.github/workflows/check-install.yml) is the exit test
of a release. Start it after the tap and the bucket hold the release:

    gh workflow run check-install.yml -f version=0.1.0

It checks the checksum and the attestation of each file, installs the command
line from the tap on macOS and on Linux, the app from the tap on macOS, and
both from the bucket on Windows. After winget-pkgs takes the manifest, add
`-f winget=true`.

## Test the workflow without a release

    gh workflow run release.yml

A run started by hand builds every file and hashes it, and it publishes
nothing. It makes no attestation either, because on a public repository an
attestation goes into the public Sigstore log. The files wait in the run as an
artifact named `dist`:

    gh run download <run-id> --name dist

## Check a download

Two checks, and they answer different questions.

The checksum says that the file arrived whole:

    shasum -a 256 -c SHA256SUMS --ignore-missing

The attestation says that this file came out of this repository, from the
commit that the tag names, through the release workflow:

    gh attestation verify onyx-0.1.0-aarch64-apple-darwin --repo Stiven-Gjekaj/OnyxDiskInfo

| Asset | What it is |
| --- | --- |
| `onyx-<version>-x86_64-unknown-linux-musl` | The command line, Linux on x86_64, any distribution |
| `onyx-<version>-aarch64-unknown-linux-musl` | The command line, Linux on arm64, any distribution |
| `onyx-<version>-x86_64-apple-darwin` | The command line, macOS on Intel |
| `onyx-<version>-aarch64-apple-darwin` | The command line, macOS on Apple silicon |
| `onyx-<version>-x86_64-pc-windows-msvc.exe` | The command line, Windows on x86_64 |
| `onyx-<version>-aarch64-pc-windows-msvc.exe` | The command line, Windows on arm64 |
| `OnyxDiskInfo-<version>-macos-universal.zip` | OnyxDiskInfo.app with `onyx` in it, macOS on each chip |
| `OnyxDiskInfo-<version>-x86_64-pc-windows-msvc.zip` | OnyxDiskInfo.exe and onyx.exe, Windows on x86_64 |
| `OnyxDiskInfo-<version>-aarch64-pc-windows-msvc.zip` | OnyxDiskInfo.exe and onyx.exe, Windows on arm64 |

No release holds the app for Linux yet. It builds from source, and the README
says how.
