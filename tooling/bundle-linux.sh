#!/usr/bin/env bash
# Packs the app for Linux into a tarball, for a release.
#
#   tooling/bundle-linux.sh [output-folder]
#
# Writes sound-tools-<version>-linux-<arch>.tar.gz to dist/ by default. It holds the program,
# a desktop file, the icon and install.sh, which puts them in ~/.local. The build folder of
# .cargo/config.toml is a macOS one, so set CARGO_TARGET_DIR first.
set -euo pipefail

output="${1:-dist}"
if [[ $# -gt 1 || "$output" == -* ]]; then
  echo "usage: tooling/bundle-linux.sh [output-folder]" >&2
  exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
mkdir -p "$output"
output="$(cd "$output" && pwd)"

version="$(tooling/version.sh)"
# x86_64 or aarch64.
name="sound-tools-$version-linux-$(uname -m)"

cargo build --release -p runtime --locked
target="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"

scratch="$(mktemp -d)"
folder="$scratch/$name"
mkdir -p "$folder"
cp "$target/release/runtime" "$folder/sound-tools"
# The release profile keeps line tables for backtraces, and on Linux they are in the program.
# They are most of its size. The names of the functions stay.
strip --strip-debug "$folder/sound-tools"
cp tooling/icon/sound-tools-512.png "$folder/sound-tools.png"
cp tooling/linux/sound-tools.desktop tooling/linux/install.sh LICENSE "$folder/"
"$folder/sound-tools" --version

archive="$output/$name.tar.gz"
tar -czf "$archive" -C "$scratch" "$name"
rm -rf "$scratch"
echo "built $archive"
