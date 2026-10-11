#!/usr/bin/env bash
# Write the Homebrew formula of one release, from the SHA256SUMS of that
# release.
#
# The formula installs the command line `onyx`, for macOS on each chip and
# for Linux on each architecture. It goes into the tap, and a person commits
# it there: no bot pushes to the tap.
#
# Usage: scripts/homebrew-formula.sh <version> <SHA256SUMS> [base-url] > onyxdiskinfo.rb
#
# The base URL defaults to the download address of the release on GitHub. A
# test gives a file:// address, so that the formula installs a local build.

set -euo pipefail
source "$(dirname "$0")/package-lib.sh"

USAGE="usage: homebrew-formula.sh <version> <SHA256SUMS> [base-url]"
VERSION="${1:?$USAGE}"
SUMS="${2:?$USAGE}"
BASE="${3:-$(release_url "$VERSION")}"

MAC_ARM="$(digest "$SUMS" "onyx-${VERSION}-aarch64-apple-darwin")"
MAC_INTEL="$(digest "$SUMS" "onyx-${VERSION}-x86_64-apple-darwin")"
LINUX_ARM="$(digest "$SUMS" "onyx-${VERSION}-aarch64-unknown-linux-musl")"
LINUX_INTEL="$(digest "$SUMS" "onyx-${VERSION}-x86_64-unknown-linux-musl")"

cat <<FORMULA
# scripts/homebrew-formula.sh in the OnyxDiskInfo repository writes this file
# from the SHA256SUMS of the release. Change the script, and not this file.
class Onyxdiskinfo < Formula
  desc "Reads the health of every disk, and tests, formats and erases it"
  homepage "https://github.com/Stiven-Gjekaj/OnyxDiskInfo"
  version "${VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "${BASE}/onyx-${VERSION}-aarch64-apple-darwin"
      sha256 "${MAC_ARM}"
    end
    on_intel do
      url "${BASE}/onyx-${VERSION}-x86_64-apple-darwin"
      sha256 "${MAC_INTEL}"
    end
  end

  on_linux do
    on_arm do
      url "${BASE}/onyx-${VERSION}-aarch64-unknown-linux-musl"
      sha256 "${LINUX_ARM}"
    end
    on_intel do
      url "${BASE}/onyx-${VERSION}-x86_64-unknown-linux-musl"
      sha256 "${LINUX_INTEL}"
    end
  end

  def install
    binary = Dir["onyx-*"].first
    chmod 0755, binary
    bin.install binary => "onyx"
  end

  def caveats
    <<~EOS
      Most hosts read the health of a drive only as root or Administrator:
        sudo onyx health <number>
    EOS
  end

  test do
    assert_match "onyx #{version}", shell_output("#{bin}/onyx --version")
  end
end
FORMULA
