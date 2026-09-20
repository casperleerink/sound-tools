# Shared brief for a step agent

You build one step of a milestone. An orchestrator reviews your pull request and merges it. You do not merge.

## Where you work

- The orchestrator created and checked out your branch. Stay on it. Do not touch `main`.
- Never use bare `git stash`. Use a WIP commit if you must set work aside.
- The Cargo target directory is shared (`.cargo/config.toml`). Never change rustflags or the target directory.
- Leave no process running when you finish. Temporary projects go under `/private/tmp`.

## Read first

1. ARCHITECTURE.md. It is the source of truth for decisions.
2. ENGINEERING.md: dependencies with versions, the engine design in section 3, testing in section 4, rules for agents in section 5.
3. The README of every crate your step touches.
4. For GPUI work: the skills in `.agents/skills/` and DESIGN.md.

`experiments/` is reference only. Do not edit it.

## How the owner wants code written

- Simple and small. Build what the step needs and nothing for later. If a simpler way exists, take it and say so in your report.
- Strong type safety comes first, before tests. Make wrong states hard to write.
- Do not duplicate types or work.
- Follow ENGINEERING.md section 5. No `unwrap()` outside tests, no `let _ =` on errors, full-word names, comments say why.
- Check the real current version and API of every crate you use. Do not write APIs from memory.
- Docs and comments in simple technical English. Short sentences.

## Docs

When you settle something the docs leave open, record it in ARCHITECTURE.md or ENGINEERING.md, briefly. When code and docs disagree, fix the doc in the same pull request. A new tool brings its doc for outside agents, and a test loads every example in it.

## Checks before you push

The list is in the root README under "Checks", plus the realtime sanitizer run from ENGINEERING.md. CI runs the same on macOS. CI has no audio device, no MIDI device, no display and no third-party plugins. Tests must need none of them. Run what needs hardware yourself and report it.

## Commit, push, pull request

- Small logical commits.
- If `gh` defaults to another account than the repo owner's, pass that account's token for each command with `GH_TOKEN`. Do not switch the active account.
- The pull request body says what changed, what you decided, how you verified it with real numbers, and what you did not verify.
- Wait for CI and fix failures before you report.

## Your final report

- The pull request URL and the CI status.
- The public API you added, one line each.
- Evidence for every verify item of your step: the command and the output that matters. Say plainly what you could not verify.
- Decisions you made that the docs had not made, and any deviation with its reason.
- Known gaps for later steps.

A reported gap is fine. A hidden one is not.
