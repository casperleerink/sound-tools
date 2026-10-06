---
name: release
description: Cut a versioned GitHub release of the Sound Tools app for macOS, Linux and Windows. Bumps the workspace version, merges it through a pull request, tags main and watches the release workflow. Use when asked to release, cut a release or ship a new version.
argument-hint: "[patch|minor|major|<version>]"
disable-model-invocation: true
---

# Release

A release is a tag `v<version>` on main. Pushing it starts `.github/workflows/release.yml`, which builds the macOS zip, the two Linux tarballs and the Windows zip and makes the GitHub release with them and their `SHA256SUMS`, which the app's updater checks. The version lives in one place, `version` in `[workspace.package]` of `Cargo.toml`. The workflow fails when the tag does not match it.

Stop and report at the first step that fails. Never move or delete a tag that was pushed.

## 1. The new version

The argument is `patch`, `minor`, `major` or a version such as `0.3.0`, and it wins. With no argument, decide from what changed since the last release:

```sh
git describe --tags --abbrev=0 origin/main     # the last release tag
gh pr list --state merged --base main --search "merged:>=<date of that tag>" --json number,title
```

- `major`: a project made with the last release no longer opens or sounds different, or something a composer used is gone.
- `minor`: something new a composer can do or see: a tool, an instrument, a view, a format field.
- `patch`: only fixes and changes nobody notices.

The largest that applies wins. Say which one and why in one line, before you bump.

`tooling/version.sh` prints the current version. `0.1.4` becomes `0.1.5` for patch, `0.2.0` for minor, `1.0.0` for major. Below, `$VERSION` is the new version, without the `v`: set `VERSION=0.1.5`.

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

Nobody pushes to main. The bump goes through a pull request. The active `gh` account may not have the right to open or merge it; the owner's does, so `gh pr create` and `gh pr merge` run with its token:

```sh
git switch -c "release/v$VERSION"
git commit -am "Release v$VERSION"
git push -u origin "release/v$VERSION"
GH_TOKEN=$(gh auth token -u casperleerink) gh pr create --base main --title "Release v$VERSION" --body "Bumps the version to $VERSION. The tag v$VERSION on the merge commit makes the release."
gh pr checks <number> --watch --fail-fast
```

When every check is green, merge:

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

`headBranch` is the tag. Report the release URL and its five files. When a build job fails for a reason outside the code, such as a runner problem, run `gh run rerun <id> --failed`. When the code is at fault, fix it in a pull request and release the next patch.

## A dry run

`gh workflow run release.yml --ref <branch>` builds the four files from any branch that has the workflow, and keeps them as artifacts of the run, with no tag and no release. `gh run download <id>` fetches them.
