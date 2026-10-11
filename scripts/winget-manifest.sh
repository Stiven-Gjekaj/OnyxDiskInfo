#!/usr/bin/env bash
# Write the winget manifest of one release, from the SHA256SUMS of that
# release.
#
# The manifest installs the Windows zip of the release, for x86_64 and for
# arm64, as two portable commands: `onyx` and `OnyxDiskInfo`. A person opens
# the pull request to winget-pkgs: no bot does.
#
# Usage: scripts/winget-manifest.sh <version> <SHA256SUMS> <directory> [base-url]
#
# The three files go into <directory>/manifests/s/Stiven-Gjekaj/OnyxDiskInfo/
# <version>/, which is where winget-pkgs keeps them.

set -euo pipefail
source "$(dirname "$0")/package-lib.sh"

USAGE="usage: winget-manifest.sh <version> <SHA256SUMS> <directory> [base-url]"
VERSION="${1:?$USAGE}"
SUMS="${2:?$USAGE}"
OUT="${3:?$USAGE}"
BASE="${4:-$(release_url "$VERSION")}"

ID="Stiven-Gjekaj.OnyxDiskInfo"
SCHEMA="1.12.0"

# winget-pkgs writes a digest in capitals. Each digest goes into a variable of
# its own first: a stop inside a nested $( ) does not stop the script.
X64="$(digest "$SUMS" "OnyxDiskInfo-${VERSION}-x86_64-pc-windows-msvc.zip")"
ARM64="$(digest "$SUMS" "OnyxDiskInfo-${VERSION}-aarch64-pc-windows-msvc.zip")"
X64="$(echo "$X64" | tr 'a-f' 'A-F')"
ARM64="$(echo "$ARM64" | tr 'a-f' 'A-F')"

DIR="${OUT}/manifests/s/Stiven-Gjekaj/OnyxDiskInfo/${VERSION}"
mkdir -p "$DIR"

cat > "${DIR}/${ID}.yaml" <<EOF2
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: ${SCHEMA}
EOF2

cat > "${DIR}/${ID}.installer.yaml" <<EOF2
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: OnyxDiskInfo\\onyx.exe
  PortableCommandAlias: onyx
- RelativeFilePath: OnyxDiskInfo\\OnyxDiskInfo.exe
  PortableCommandAlias: OnyxDiskInfo
Installers:
- Architecture: x64
  InstallerUrl: ${BASE}/OnyxDiskInfo-${VERSION}-x86_64-pc-windows-msvc.zip
  InstallerSha256: ${X64}
- Architecture: arm64
  InstallerUrl: ${BASE}/OnyxDiskInfo-${VERSION}-aarch64-pc-windows-msvc.zip
  InstallerSha256: ${ARM64}
ManifestType: installer
ManifestVersion: ${SCHEMA}
EOF2

cat > "${DIR}/${ID}.locale.en-US.yaml" <<EOF2
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
PackageLocale: en-US
Publisher: Stiven Gjekaj
PublisherUrl: https://github.com/Stiven-Gjekaj
PackageName: OnyxDiskInfo
PackageUrl: https://github.com/Stiven-Gjekaj/OnyxDiskInfo
License: MIT
LicenseUrl: https://github.com/Stiven-Gjekaj/OnyxDiskInfo/blob/main/LICENSE
ShortDescription: Reads the health of every disk, and tests, formats and erases it
Description: OnyxDiskInfo reads the SMART and NVMe health of each drive, and measures a drive that reports nothing. It scans, benchmarks, and tests drives, and it formats and erases them. It keeps a history of each drive and gives an alert when a drive gets worse. It refuses the disk that the system starts from.
Tags:
- smart
- nvme
- disk
- health
- format
ManifestType: defaultLocale
ManifestVersion: ${SCHEMA}
EOF2

echo "$DIR"
