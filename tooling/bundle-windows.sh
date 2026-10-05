#!/usr/bin/env bash
# Packs the app for Windows into a zip, for a release. Run it on Windows in Git Bash, as the
# release job does.
#
#   tooling/bundle-windows.sh [output-folder]
#
# Writes sound-tools-<version>-windows-x86_64.zip to dist/ by default. It holds a folder with
# the program, LICENSE and install.ps1, which puts the program in
# %LOCALAPPDATA%\Programs\Sound Tools. The program is the whole app: the plugin scan runs it
# again, and the icon is inside it.
set -euo pipefail

output="${1:-dist}"
if [[ $# -gt 1 || "$output" == -* ]]; then
  echo "usage: tooling/bundle-windows.sh [output-folder]" >&2
  exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
mkdir -p "$output"
output="$(cd "$output" && pwd)"

version="$(tooling/version.sh)"
name="sound-tools-$version-windows-x86_64"

cargo build --release -p runtime --locked
target="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
# JSON writes each backslash of C:\a\target twice. Bash takes C:/a/target.
target="${target//\\\\//}"

scratch="$(mktemp -d)"
folder="$scratch/$name"
mkdir -p "$folder"
cp "$target/release/runtime.exe" "$folder/sound-tools.exe"
cp tooling/windows/install.ps1 LICENSE "$folder/"
"$folder/sound-tools.exe" --version

archive="$output/$name.zip"
rm -f "$archive"
# The tar of Windows writes zips. The GNU tar of Git Bash does not, and there is no zip.
"$(cygpath "$SYSTEMROOT")/System32/tar.exe" -a -cf "$archive" -C "$scratch" "$name"
rm -rf "$scratch"
echo "built $archive"
