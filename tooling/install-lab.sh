#!/usr/bin/env bash
# Builds this branch as Sound Tools Lab.app and puts it in /Applications, next to Sound Tools.
#
#   tooling/install-lab.sh
#
# Run it again to update the lab. It is its own app to macOS: its own name, bundle id and
# microphone permission. It is built with SOUND_TOOLS_NO_UPDATES, so it never updates itself
# from a release, nor installs an update Sound Tools downloaded. Its agent runs its own
# `sound-tools`, which is first on the agent's PATH. The two share the support folder:
# recent projects, agents and their threads.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

build="$(mktemp -d)"
trap 'rm -rf "$build"' EXIT
SOUND_TOOLS_NO_UPDATES=1 tooling/bundle-macos.sh "$build"

app="$build/Sound Tools Lab.app"
mv "$build/Sound Tools.app" "$app"
plist="$app/Contents/Info.plist"
plutil -replace CFBundleName -string "Sound Tools Lab" "$plist"
plutil -replace CFBundleDisplayName -string "Sound Tools Lab" "$plist"
plutil -replace CFBundleIdentifier -string com.casperleerink.sound-tools-lab "$plist"
plutil -replace CFBundleVersion -string "$(git rev-parse --short HEAD)" "$plist"
codesign --force --deep --sign - --entitlements tooling/macos/entitlements.plist "$app"
codesign --verify --deep --strict "$app"

installed="/Applications/Sound Tools Lab.app"
rm -rf "$installed"
ditto "$app" "$installed"
echo "installed $installed ($(git rev-parse --short HEAD))"
if pgrep -qf "$installed/Contents/MacOS/"; then
  echo "Sound Tools Lab is open: quit and open it again to use this build."
fi
