#!/usr/bin/env bash
# Write the Scoop manifest of one release, from the SHA256SUMS of that
# release.
#
# The manifest installs the Windows zip of the release, for x86_64 and for
# arm64: `onyx` goes on the PATH, and the app gets a shortcut. A person puts
# it into the bucket and commits it there: no bot pushes to a bucket.
#
# Usage: scripts/scoop-manifest.sh <version> <SHA256SUMS> [base-url] > onyxdiskinfo.json

set -euo pipefail
source "$(dirname "$0")/package-lib.sh"

USAGE="usage: scoop-manifest.sh <version> <SHA256SUMS> [base-url]"
VERSION="${1:?$USAGE}"
SUMS="${2:?$USAGE}"
BASE="${3:-$(release_url "$VERSION")}"

X64="$(digest "$SUMS" "OnyxDiskInfo-${VERSION}-x86_64-pc-windows-msvc.zip")"
ARM64="$(digest "$SUMS" "OnyxDiskInfo-${VERSION}-aarch64-pc-windows-msvc.zip")"

# The address of a later release, for the autoupdate of Scoop. Scoop puts the
# version where $version stands, and it hashes the file itself.
LATER='https://github.com/Stiven-Gjekaj/OnyxDiskInfo/releases/download/v$version/OnyxDiskInfo-$version'

cat <<EOF2
{
    "version": "${VERSION}",
    "description": "Reads the health of every disk, and tests, formats and erases it",
    "homepage": "https://github.com/Stiven-Gjekaj/OnyxDiskInfo",
    "license": "MIT",
    "notes": "Onyx reads most drives only from a shell that runs as Administrator. The app asks for it when it starts.",
    "architecture": {
        "64bit": {
            "url": "${BASE}/OnyxDiskInfo-${VERSION}-x86_64-pc-windows-msvc.zip",
            "hash": "${X64}"
        },
        "arm64": {
            "url": "${BASE}/OnyxDiskInfo-${VERSION}-aarch64-pc-windows-msvc.zip",
            "hash": "${ARM64}"
        }
    },
    "extract_dir": "OnyxDiskInfo",
    "bin": "onyx.exe",
    "shortcuts": [["OnyxDiskInfo.exe", "OnyxDiskInfo"]],
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "${LATER}-x86_64-pc-windows-msvc.zip"
            },
            "arm64": {
                "url": "${LATER}-aarch64-pc-windows-msvc.zip"
            }
        }
    }
}
EOF2
