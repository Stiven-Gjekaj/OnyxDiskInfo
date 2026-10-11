#!/usr/bin/env bash
# Write the Homebrew cask of one release, from the SHA256SUMS of that
# release.
#
# The cask installs OnyxDiskInfo.app, which runs on Apple silicon and on
# Intel. A person puts it into the tap and commits it there: no bot pushes to
# the tap.
#
# Usage: scripts/homebrew-cask.sh <version> <SHA256SUMS> [base-url] > onyxdiskinfo.rb

set -euo pipefail
source "$(dirname "$0")/package-lib.sh"

USAGE="usage: homebrew-cask.sh <version> <SHA256SUMS> [base-url]"
VERSION="${1:?$USAGE}"
SUMS="${2:?$USAGE}"
BASE="${3:-https://github.com/Stiven-Gjekaj/OnyxDiskInfo/releases/download/v#{version}}"

FILE="OnyxDiskInfo-${VERSION}-macos-universal.zip"
SHA="$(digest "$SUMS" "$FILE")"

cat <<CASK
# scripts/homebrew-cask.sh in the OnyxDiskInfo repository writes this file
# from the SHA256SUMS of the release. Change the script, and not this file.
cask "onyxdiskinfo" do
  version "${VERSION}"
  sha256 "${SHA}"

  url "${BASE}/OnyxDiskInfo-#{version}-macos-universal.zip"
  name "OnyxDiskInfo"
  desc "Reads the health of every disk, and tests, formats and erases it"
  homepage "https://github.com/Stiven-Gjekaj/OnyxDiskInfo"

  depends_on macos: ">= :big_sur"

  app "OnyxDiskInfo.app"

  zap trash: "~/Library/Application Support/OnyxDiskInfo"

  caveats <<~EOS
    No paid certificate signed OnyxDiskInfo, so macOS can refuse to open it.
    Take the mark off after the install:

      xattr -dr com.apple.quarantine #{appdir}/OnyxDiskInfo.app

    OnyxDiskInfo keeps the history of each drive in
    ~/Library/Application Support/OnyxDiskInfo/history.sqlite.
    An uninstall does not remove it. "brew uninstall --zap" does.
  EOS
end
CASK
