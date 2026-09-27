#!/usr/bin/env bash
# Prints the version of the app: `version` in [workspace.package] of Cargo.toml.
# It is the one place the version is written. A release tag is `v` and this.
set -euo pipefail

cd "$(dirname "$0")/.."
version="$(sed -n '/^\[workspace.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "no version in [workspace.package] of Cargo.toml" >&2
  exit 1
fi
echo "$version"
