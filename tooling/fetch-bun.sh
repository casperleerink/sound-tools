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

version="1.4.3"

if [[ $# -ne 2 || "$1" == -* ]]; then
  echo "usage: tooling/fetch-bun.sh <folder> <license-folder>" >&2
  exit 2
fi
folder="$1"
license_folder="$2"
license_sum="056696884250b0d682365260cf1487a6501b1665a343ec60e23a1e647043c572"

# The machines the bundle scripts build on. On x86_64 the baseline build, which runs on a
# processor without AVX2 too.
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset="bun-darwin-aarch64" sum="80afd4c069b46a6fa8f3ec520bebc750a7843e4a8d5b9583d29ad436c00a3403" ;;
  Darwin-x86_64) asset="bun-darwin-x64-baseline" sum="0fde04e8b092156b97fe6432e899baf16d9ed6ce52cbb4218d2d9c07a2f827ef" ;;
  Linux-x86_64) asset="bun-linux-x64-baseline" sum="1fc2edac843102909e3a1be1d8d9802cc6071cf074e67e88231f4ffe0f8b397b" ;;
  Linux-aarch64) asset="bun-linux-aarch64" sum="efa9813da5ed72423bf847f916e8d2c47c0d776add972354026a75e10da9aa21" ;;
  MINGW*-x86_64 | MSYS*-x86_64) asset="bun-windows-x64-baseline" sum="d7a7d3eb216614544aab0cde2d8d9f2e4e87839bf5a30a1cd11d077f02d84e0c" ;;
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

# The zip holds <asset>/bun. On Windows the tar of Windows unpacks it.
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
