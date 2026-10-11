#!/usr/bin/env bash
# Run each package script on a SHA256SUMS that this script makes, and check
# the shape of what each one writes. The digests are made up, and nothing is
# downloaded or published.
#
# Usage: scripts/check-packaging.sh

set -euo pipefail

cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

version="9.8.7"
for file in \
    "onyx-${version}-x86_64-unknown-linux-musl" \
    "onyx-${version}-aarch64-unknown-linux-musl" \
    "onyx-${version}-x86_64-apple-darwin" \
    "onyx-${version}-aarch64-apple-darwin" \
    "onyx-${version}-x86_64-pc-windows-msvc.exe" \
    "onyx-${version}-aarch64-pc-windows-msvc.exe" \
    "OnyxDiskInfo-${version}-macos-universal.zip" \
    "OnyxDiskInfo-${version}-x86_64-pc-windows-msvc.zip" \
    "OnyxDiskInfo-${version}-aarch64-pc-windows-msvc.zip"; do
    printf '%064x  %s\n' "${#file}" "$file" >> "$work/SHA256SUMS"
done

scripts/homebrew-formula.sh "$version" "$work/SHA256SUMS" > "$work/formula.rb"
grep -q 'class Onyxdiskinfo < Formula' "$work/formula.rb"
test "$(grep -c 'sha256 "' "$work/formula.rb")" = 4
grep -q "bin.install binary => \"onyx\"" "$work/formula.rb"

scripts/homebrew-cask.sh "$version" "$work/SHA256SUMS" > "$work/cask.rb"
grep -q 'cask "onyxdiskinfo" do' "$work/cask.rb"
grep -q "version \"${version}\"" "$work/cask.rb"
grep -q 'app "OnyxDiskInfo.app"' "$work/cask.rb"

if command -v ruby >/dev/null 2>&1; then
    ruby -c "$work/formula.rb" > /dev/null
    ruby -c "$work/cask.rb" > /dev/null
fi

scripts/scoop-manifest.sh "$version" "$work/SHA256SUMS" > "$work/scoop.json"
python3 - "$work/scoop.json" "$version" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
assert m["version"] == sys.argv[2], m["version"]
assert m["bin"] == "onyx.exe"
assert m["extract_dir"] == "OnyxDiskInfo"
for arch in ("64bit", "arm64"):
    a = m["architecture"][arch]
    assert a["url"].endswith(".zip") and len(a["hash"]) == 64, a
PY

dir="$(scripts/winget-manifest.sh "$version" "$work/SHA256SUMS" "$work/winget")"
test "$(ls "$dir" | wc -l | tr -d ' ')" = 3
grep -q "PackageVersion: ${version}" "$dir/Stiven-Gjekaj.OnyxDiskInfo.installer.yaml"
grep -q 'RelativeFilePath: OnyxDiskInfo\\onyx.exe' "$dir/Stiven-Gjekaj.OnyxDiskInfo.installer.yaml"
test "$(grep -c 'InstallerSha256: [0-9A-F]\{64\}$' "$dir/Stiven-Gjekaj.OnyxDiskInfo.installer.yaml")" = 2

# A SHA256SUMS that lacks a file stops each script, with the name of the file.
grep -v 'apple-darwin\|macos\|windows' "$work/SHA256SUMS" > "$work/SHORT"
for script in homebrew-formula homebrew-cask scoop-manifest; do
    if scripts/$script.sh "$version" "$work/SHORT" > /dev/null 2> "$work/error"; then
        echo "$script.sh took a SHA256SUMS with a file missing" >&2
        exit 1
    fi
    grep -q "holds no digest for" "$work/error"
done
if scripts/winget-manifest.sh "$version" "$work/SHORT" "$work/w2" > /dev/null 2> "$work/error"; then
    echo "winget-manifest.sh took a SHA256SUMS with a file missing" >&2
    exit 1
fi

echo "Each package script wrote a file of the right shape."
