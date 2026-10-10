#!/usr/bin/env bash
# Puts the official Bun program for this computer in a folder, and its license, which also
# covers the libraries it links, in another. The bundle scripts ship them with the app's
# program, so the tools of a project run with no Bun installed. This is the one place that
# names its version.
#
#   tooling/fetch-bun.sh <folder> <license-folder>
#
# The zip comes from the oven-sh/bun release of that version and must match the sha256 written
# here, taken from that release's SHASUMS256.txt, before anything is taken out of it: a release
# changed after the fact does not get in. A new version is a new version and new sums here.
set -euo pipefail

version="1.4.2"

if [[ $# -ne 2 || "$1" == -* ]]; then
  echo "usage: tooling/fetch-bun.sh <folder> <license-folder>" >&2
  exit 2
fi
folder="$1"
license_folder="$2"
license_sum="b9caf52728691b4057e371232c221a132883198be2f3d2ddf92c90404c984b1a"

# The machines the bundle scripts build on. On x86_64 the baseline build, which runs on a
# processor without AVX2 too.
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset="bun-darwin-aarch64" sum="90987a3a16d7db556d886ac3d551e7b6d3edf0a1cf43acaed622e8676be1d12f" ;;
  Darwin-x86_64) asset="bun-darwin-x64-baseline" sum="bad5bbd6cf14d0980d115f5954c9ff904df619d5e994d2da1ffccd3f316300b0" ;;
  Linux-x86_64) asset="bun-linux-x64-baseline" sum="c678040f14fe0440eb839d37cbd0ce4c051a32da72806ac97de6a6aab6bf728f" ;;
  Linux-aarch64) asset="bun-linux-aarch64" sum="54328bbc2d9c8e0c9f892c544d66c57a83b84139e34909e5ee81758f1ac8fda7" ;;
  MINGW*-x86_64 | MSYS*-x86_64) asset="bun-windows-x64-baseline" sum="78c221c2376f79731ccf4e4af0b3bb46d81fefa3296c5abee09ad8a1b21e68c6" ;;
  *) echo "error: no Bun for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
fetch() {
  curl --fail --silent --show-error --location --retry 3 -o "$scratch/$1" "$2"
}
# Stops unless the file in the scratch folder has the sum. macOS has no sha256sum.
check() {
  local actual
  if command -v sha256sum >/dev/null; then
    actual="$(sha256sum "$scratch/$1")"
  else
    actual="$(shasum -a 256 "$scratch/$1")"
  fi
  if [[ "${actual%% *}" != "$2" ]]; then
    echo "error: $1 of Bun $version is not the one this script was written for" >&2
    exit 1
  fi
}
fetch "$asset.zip" "https://github.com/oven-sh/bun/releases/download/bun-v$version/$asset.zip"
check "$asset.zip" "$sum"
fetch LICENSE.md "https://raw.githubusercontent.com/oven-sh/bun/bun-v$version/LICENSE.md"
check LICENSE.md "$license_sum"

# The zip holds <asset>/bun. On Windows the tar of Windows unpacks it, as in bundle-windows.sh.
if [[ "$asset" == bun-windows-* ]]; then
  program="bun.exe"
  "$(cygpath "$SYSTEMROOT")/System32/tar.exe" -xf "$scratch/$asset.zip" -C "$scratch"
else
  program="bun"
  unzip -q "$scratch/$asset.zip" -d "$scratch"
fi
mkdir -p "$folder" "$license_folder"
cp "$scratch/$asset/$program" "$folder/$program"
cp "$scratch/LICENSE.md" "$license_folder/bun-LICENSE.md"
echo "bun $("$folder/$program" --version) in $folder"
