#!/usr/bin/env bash
# Deletes the build folder of every worktree of this repository that nobody used for a while.
#
#   tooling/clean-builds.sh [days]
#
# Every worktree builds into its own target/, about 5 GB each. A worktree counts as used when git
# touched its index, which a checkout, a commit or a status does. Default: 2 days. The worktree
# itself stays; its next build is a full one.
set -euo pipefail

days="${1:-2}"
if [[ $# -gt 1 || ! "$days" =~ ^[0-9]+$ ]]; then
  echo "usage: tooling/clean-builds.sh [days]" >&2
  exit 2
fi

git worktree list --porcelain | sed -n 's/^worktree //p' | while read -r worktree; do
  target="$worktree/target"
  index="$(git -C "$worktree" rev-parse --absolute-git-dir)/index"
  [[ -d "$target" && -f "$index" ]] || continue
  if [[ -z "$(find "$index" -mtime "-$days")" ]]; then
    echo "$(du -sh "$target" | cut -f1)	$target"
    rm -rf "$target"
  fi
done
