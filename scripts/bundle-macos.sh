#!/bin/sh
# Build OnyxDiskInfo.app, the form of the app that macOS shows in the Dock and
# in Finder with its icon and its name.
#
# The bundle holds the app and the command line `onyx` side by side, because
# the button that reads a drive as administrator runs `onyx` from the folder
# of the app.
#
# Usage: scripts/bundle-macos.sh <OnyxDiskInfo binary> <onyx binary> <out dir>
#
# The bundle goes to <out dir>/OnyxDiskInfo.app.
set -eu

usage="usage: bundle-macos.sh <OnyxDiskInfo binary> <onyx binary> <out dir>"
app_binary=${1:?$usage}
cli_binary=${2:?$usage}
out=${3:?$usage}

root=$(cd "$(dirname "$0")/.." && pwd)
version=$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" \
    | python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "onyx-gui"))')

app="$out/OnyxDiskInfo.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$app_binary" "$app/Contents/MacOS/OnyxDiskInfo"
cp "$cli_binary" "$app/Contents/MacOS/onyx"
cp "$root/assets/icons/onyx.icns" "$app/Contents/Resources/onyx.icns"
sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

echo "Built $app, version $version."
