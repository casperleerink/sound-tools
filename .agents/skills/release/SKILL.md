---
name: release
description: Cut a versioned GitHub release of the Sound Tools app for macOS, Linux and Windows. Picks the version, tags main and watches the release workflow. Use when asked to release, cut a release or ship a new version.
argument-hint: "[patch|minor|major|<version>]"
disable-model-invocation: true
---

# Release

A release is a tag `v<version>` on main, and the tag is the only place the version is written. Pushing it starts `.github/workflows/release.yml`, which builds the macOS zip, the two Linux tarballs and the Windows setup program with that version and makes the GitHub release with them and their `SHA256SUMS`, which the app's updater checks. Nothing is committed: `version` in `Cargo.toml` stays `0.0.0`.

Stop and report at the first step that fails. Never move or delete a tag that was pushed.

## 1. The new version

The argument is `patch`, `minor`, `major` or a version such as `0.3.0`, and it wins. With no argument, decide from what changed since the last release:

```sh
git fetch origin --tags
git describe --tags --abbrev=0 origin/main     # the last release tag
gh pr list --state merged --base main --search "merged:>=<date of that tag>" --json number,title
```

- `major`: a project made with the last release no longer opens or sounds different, or something a composer used is gone.
- `minor`: something new a composer can do or see: a tool, an instrument, a view, a format field.
- `patch`: only fixes and changes nobody notices.

The largest that applies wins. Say which one and why in one line, before you tag.

`v0.1.4` becomes `0.1.5` for patch, `0.2.0` for minor, `1.0.0` for major. Below, `$VERSION` is the new version, without the `v`: set `VERSION=0.1.5`.

## 2. Check

Tag the head of main, once main's full CI run on it ended `success`. A pull request runs only part of CI.

```sh
sha=$(git rev-parse origin/main)
gh run list --workflow ci.yml --branch main --commit "$sha" --json databaseId,status,conclusion
git ls-remote --tags origin "v$VERSION"       # must print nothing
```

If the run still goes, wait with `gh run watch <id> --exit-status`.

## 3. Tag

```sh
git tag -a "v$VERSION" "$sha" -m "Sound Tools $VERSION"
git push origin "v$VERSION"
```

## 4. Watch

```sh
gh run list --workflow release.yml --event push --limit 1 --json databaseId,headBranch
gh run watch <id> --exit-status
gh release view "v$VERSION" --json url --jq .url
```

`headBranch` is the tag. Report the release URL and its five files. When a build job fails for a reason outside the code, such as a runner problem, run `gh run rerun <id> --failed`. When the code is at fault, fix it in a pull request and release the next patch.

## A dry run

`gh workflow run release.yml --ref <branch>` builds the four files from any branch that has the workflow, and keeps them as artifacts of the run, as version 0.0.0, with no tag and no release. `gh run download <id>` fetches them.
