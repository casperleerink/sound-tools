#!/usr/bin/env bash
# Puts the official Bun program for this computer in a folder. The bundle scripts ship it next
# to the app's program, so the tools of a project run with no Bun installed. This is the one
# place that names its version.
#
#   tooling/fetch-bun.sh <folder>
#
# The zip comes from the oven-sh/bun release of that version and must match the sha256 in the
# release's SHASUMS256.txt before anything is taken out of it.
set -euo pipefail

version="1.4.2"

if [[ $# -ne 1 || "$1" == -* ]]; then
  echo "usage: tooling/fetch-bun.sh <folder>" >&2
  exit 2
fi
folder="$1"

# The machines the bundle scripts build on. On x86_64 the baseline build, which runs on a
# processor without AVX2 too.
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset="bun-darwin-aarch64" ;;
  Darwin-x86_64) asset="bun-darwin-x64-baseline" ;;
  Linux-x86_64) asset="bun-linux-x64-baseline" ;;
  Linux-aarch64) asset="bun-linux-aarch64" ;;
  MINGW*-x86_64 | MSYS*-x86_64) asset="bun-windows-x64-baseline" ;;
  *) echo "error: no Bun for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

release="https://github.com/oven-sh/bun/releases/download/bun-v$version"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
for file in "$asset.zip" SHASUMS256.txt; do
  curl --fail --silent --show-error --location --retry 3 -o "$scratch/$file" "$release/$file"
done

expected="$(awk -v name="$asset.zip" '$2 == name { print $1 }' "$scratch/SHASUMS256.txt")"
# macOS has no sha256sum.
if command -v sha256sum >/dev/null; then
  actual="$(sha256sum "$scratch/$asset.zip")"
else
  actual="$(shasum -a 256 "$scratch/$asset.zip")"
fi
actual="${actual%% *}"
if [[ -z "$expected" || "$actual" != "$expected" ]]; then
  echo "error: $asset.zip of Bun $version does not match the release's SHASUMS256.txt" >&2
  exit 1
fi

# The zip holds <asset>/bun. On Windows the tar of Windows unpacks it, as in bundle-windows.sh.
if [[ "$asset" == bun-windows-* ]]; then
  program="bun.exe"
  "$(cygpath "$SYSTEMROOT")/System32/tar.exe" -xf "$scratch/$asset.zip" -C "$scratch"
else
  program="bun"
  unzip -q "$scratch/$asset.zip" -d "$scratch"
fi
mkdir -p "$folder"
cp "$scratch/$asset/$program" "$folder/$program"
echo "bun $("$folder/$program" --version) in $folder"
