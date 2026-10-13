#!/usr/bin/env bash
# Packs the app for Windows into its setup program, for a release. Run it on Windows in Git
# Bash, as the release job does. It needs Inno Setup 6.
#
#   tooling/bundle-windows.sh [output-folder]
#
# Writes Sound-Tools-Setup-<version>.exe to dist/ by default, from tooling/windows/setup.iss. It
# installs the program, bun.exe and the licenses in %LOCALAPPDATA%\Programs\Sound Tools. The
# program is the whole app but for Bun, which runs the tools of a project: the plugin scan runs
# the program again, and the icon is inside it.
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

iscc="$(command -v iscc || true)"
if [[ -z "$iscc" ]]; then
  iscc="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
fi
if [[ ! -x "$iscc" ]]; then
  echo "error: Inno Setup 6 is not installed: no ISCC.exe on the PATH or in its default folder" >&2
  exit 1
fi

cargo build --release -p runtime --locked
target="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
# JSON writes each backslash of C:\a\target twice. Bash takes C:/a/target.
target="${target//\\\\//}"

scratch="$(mktemp -d)"
cp "$target/release/runtime.exe" "$scratch/sound-tools.exe"
tooling/fetch-bun.sh "$scratch" "$scratch"
cp LICENSE "$scratch/"
"$scratch/sound-tools.exe" --version

setup="$output/Sound-Tools-Setup-$version.exe"
rm -f "$setup"
# Git Bash would rewrite an argument that starts with a slash into a path.
MSYS2_ARG_CONV_EXCL='*' "$iscc" /Q "/DVersion=$version" "/DSource=$(cygpath -w "$scratch")" \
  "/DOutput=$(cygpath -w "$output")" 'tooling\windows\setup.iss'
rm -rf "$scratch"
echo "built $setup"
