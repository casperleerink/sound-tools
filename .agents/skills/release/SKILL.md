---
name: release
description: Cut a versioned GitHub release of the Sound Tools app for macOS and Linux. Bumps the workspace version, merges it through a pull request, tags main and watches the release workflow. Use when asked to release, cut a release or ship a new version.
argument-hint: "[patch|minor|major|<version>]"
disable-model-invocation: true
---

# Release

A release is a tag `v<version>` on main. Pushing it starts `.github/workflows/release.yml`, which builds the macOS zip and the two Linux tarballs and makes the GitHub release. The version lives in one place, `version` in `[workspace.package]` of `Cargo.toml`. The workflow fails when the tag does not match it.

Stop and report at the first step that fails. Never move or delete a tag that was pushed.

## 1. The new version

The argument is `patch`, `minor`, `major` or a version such as `0.3.0`. No argument means `patch`. `tooling/version.sh` prints the current version. `0.1.4` becomes `0.1.5` for patch, `0.2.0` for minor, `1.0.0` for major.

## 2. Check

```sh
git fetch origin --tags
git status --porcelain                        # must print nothing
git switch main && git merge --ff-only origin/main
gh run list --workflow ci.yml --branch main --commit "$(git rev-parse HEAD)" --json status,conclusion
git ls-remote --tags origin "v$VERSION"       # must print nothing
```

CI must be `completed` and `success` for the head of main. If it still runs, wait with `gh run watch <id> --exit-status`.

## 3. Bump

Set `version = "<version>"` under `[workspace.package]` in `Cargo.toml`. Then:

```sh
cargo update --workspace
tooling/version.sh                            # prints the new version
git diff --stat                               # Cargo.toml and Cargo.lock only
```

## 4. Merge the bump

Nobody pushes to main. The bump goes through a pull request:

```sh
git switch -c "release/v$VERSION"
git commit -am "Release v$VERSION"
git push -u origin "release/v$VERSION"
gh pr create --base main --title "Release v$VERSION" --body "Bumps the version to $VERSION. The tag v$VERSION on the merge commit makes the release."
gh pr checks <number> --watch --fail-fast
```

When every check is green, merge. The active `gh` account may not have the right to merge; the owner's does:

```sh
GH_TOKEN=$(gh auth token -u casperleerink) gh pr merge <number> --merge
```

## 5. Tag

Tag the merge commit, not a commit of the branch:

```sh
sha=$(gh pr view <number> --json mergeCommit --jq .mergeCommit.oid)
git fetch origin
git switch main && git merge --ff-only origin/main
git tag -a "v$VERSION" "$sha" -m "Sound Tools $VERSION"
git push origin "v$VERSION"
```

## 6. Watch

```sh
gh run list --workflow release.yml --event push --limit 1 --json databaseId,headBranch
gh run watch <id> --exit-status
gh release view "v$VERSION" --json url --jq .url
```

`headBranch` is the tag. Report the release URL and its three files. When a build job fails for a reason outside the code, such as a runner problem, run `gh run rerun <id> --failed`. When the code is at fault, fix it in a pull request and release the next patch.

## A dry run

`gh workflow run release.yml --ref <branch>` builds the three files from any branch that has the workflow, and keeps them as artifacts of the run, with no tag and no release. `gh run download <id>` fetches them.
