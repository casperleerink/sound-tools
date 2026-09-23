# Milestone 3: mixer, built-in effects, reliable plugins and a better window

Drafted September 22, 2026. This is the plan an orchestrating agent works from. It holds goals, decisions and checks, no implementation. [ARCHITECTURE.md](../ARCHITECTURE.md) stays the source of truth. Each step records what it settles there. The starting point is "Known gaps after the second milestone" in the same file.

## Goal

A composer on a laptop mixes a piece without leaving the window. Every track has a clear volume control and a meter, solo and mute, and the master never clips. Four built-in effects (filter, compressor, EQ and reverb) cover what most tracks need without a third-party plugin. Plugins with latency stay in time with everything else, and VST 3 plugins behave the way they do in other hosts. The track panel looks like one product: the same knobs, the same cards, aligned, with a design language of its own. Editing that still needs a file edit today can be done in the window.

## Decided

Design and UI:

- The target is a normal laptop: a 13 to 14 inch screen, a trackpad, the keyboard, and no MIDI keyboard or mouse needed.
- Design comes first. The first step reviews every view, proposes a design language, and the owner approves it from screenshots before components are rebuilt. DESIGN.md may change, including the colours and the "Quiet rule", if the owner agrees.
- One set of components in `crates/ui` for every control in the track panel: knob, fader, meter, toggle, device card, card header. The synth, the plugin card, the effect cards and the mixer section all use them. The gallery shows every one.
- The repeated knob gesture code (begin, publish, finish, cancel) becomes one shared helper.

Mixer:

- The master output gets a limiter, on by default, so the output never goes over its ceiling. It is saved in the project.
- A track gets solo next to mute. Solo is saved in the track record like mute, and it is one undo step.
- Track volume gets a fader and a meter. The master gets the same, next to the limiter.
- Still no sends, buses, automation or a separate mixer view.

Built-in effects:

- Four effects, one pull request each: Filter, Compressor, EQ and Reverb. Their controls and behaviour are modelled on Ableton's Auto Filter, Compressor, EQ Eight and Reverb. The look is ours and uses the shared components. We do not copy Ableton's names, graphics or text.
- Each is a bundled extension on the SDK, like Tone. It sits in an effect slot of the track rack like an effect plugin, saves its parameters in the project, and an outside agent can edit them by file. Each brings its agent doc.
- An effect with latency (a compressor with lookahead, for example) reports it through the same path as a plugin.
- No sidechain for now.

Plugins:

- Latency compensation for the whole project: every track reaches the master in time, whatever latency its instrument and effects report, and when a plugin changes its latency while it runs.
- Playing live through a track gets no more delay than that track's own chain. A recorded take lands where the player heard it.
- VST 3 reliability: act on the restart flags we only report today (latency, parameter values, I/O changes and the MIDI CC mapping), forward keys to plugin views, keep a plugin window above the main window, let it resize, remember where it was and whether it was open, fix the scan cache key and concurrent writes to the cache, and find a plugin installed while the app runs without a restart.
- A plugin that crashes still takes the app down. A sandbox is still out.

Editing in the window:

- These become window actions, each through the one editing path, with undo and keys: reorder the rack by dragging, rename a track, add or remove a tempo change, copy and paste, multi-select, adjustable snap, and a velocity lane.

Rules that stay:

- The core knows no notes, MIDI, plugins or effects. All of this is extensions on the SDK, and extensions do not depend on each other.
- CI needs no third-party plugin, no MIDI device, no audio device and no display.
- Every edit goes through the one editing path, with undo. Outside file edits keep applying live.
- The realtime checks pass on every `process` function of our own code.

## Out of scope

Audio recording, the in-app agent and the outer application, AU, a plugin sandbox, automation, sends and buses, a separate mixer view, sidechain, loop playback, tempo ramps, time signature changes.

## Steps, one pull request each

| # | Step | Done when |
| --- | --- | --- |
| 0 | UI review and design direction | Screenshots of every view at laptop size, a list of what is wrong (alignment, spacing, mixed controls, no clear identity), and a proposed design language in DESIGN.md. The owner has approved it. |
| 1 | Shared components and the track panel redone | Knob, fader, meter, toggle and device card live in `crates/ui` and the gallery. The synth, plugin cards, effect cards and mixer section all use them, aligned. One knob gesture helper. Snapshots before and after. |
| 2 | Mixer: master limiter, volume fader and meter, solo | The master never goes over its ceiling on a project that clipped before. Solo and volume work from the window and from a file, live, each as one undo step. Old projects still open. |
| 3 | Latency compensation | A test plugin with a known latency on one track and none on another: both reach the master sample-aligned, also after the latency changes during playback. Recording still lands where it was played. |
| 4 | VST 3 reliability | Each item under "Plugins" above is done or has a stated reason not to be, with a test where CI allows and a check by hand where it does not. |
| 5 | Filter | See "Verify the milestone". |
| 6 | Compressor | See "Verify the milestone". |
| 7 | EQ | See "Verify the milestone". |
| 8 | Reverb | See "Verify the milestone". |
| 9 | Editing in the window | Each action under "Editing in the window" works with the trackpad and keys, is one undo step, and a file edit of the same thing still applies live. |
| 10 | Milestone check | Every check below has evidence, the README is current, and the known gaps are listed. |

Steps 0 and 1 go first, because the mixer and the effects build on the components. Steps 5 to 8 are independent of each other and may run in parallel once step 3 is merged. Steps 1, 4 and 9 may each split in two.

## Verify the milestone

- The track panel, the mixer section and every effect use the same components. Snapshots at laptop size show them aligned.
- A project that clipped before plays and renders under the limiter's ceiling. Solo gives the same render as muting every other track.
- Filter: its measured response matches the chosen cutoff, resonance and slope. EQ: its measured curve matches the band settings. Compressor: its measured gain matches threshold, ratio and knee on a steady tone, and attack and release are within a stated bound. Reverb: renders are repeatable and its decay time matches the setting within a stated bound.
- Every built-in effect restores its parameters after close and reopen, and an outside agent changes one by file while it plays.
- Latency compensation: tracks with and without latency line up at the master in an offline render, to the sample.
- A VST 3 plugin that changes its latency or parameters while it runs is handled without reopening the project.
- An outside agent still adds a part and a track as in milestone 1, reading only the docs it needs.
- Everything from milestones 1 and 2 still passes.

## What the owner checks

No agent can hear or judge the look. The orchestrator asks the owner for these and goes on with other work meanwhile:

- After step 0: approve the design direction from the screenshots, or change it.
- After step 1: use the track panel on the laptop. Does it look like one product and feel right with a trackpad?
- After step 2: does the limiter sound transparent at normal levels? Do solo and the faders behave as expected?
- After each effect: compare it with the Ableton effect it is modelled on. Does it sound and respond right?
- After step 4: load the VST 3 plugins you use. Do their windows, keys and state behave?
- After step 9: edit a small piece only in the window. What still needs a file edit or feels slow?

## How to run it

The same way as milestone 2:

1. The orchestrator creates a branch from `main` for the step and starts one step agent with [the shared brief](agent-brief.md) and a step brief: context, goal, decided constraints, deliverables, how to verify. No implementation.
2. The step agent builds, runs every check, opens the pull request and waits for green CI.
3. The orchestrator starts a second agent with no shared context as a reviewer. It reviews only, tries to reproduce what it finds, and ranks findings as blocker, should or later.
4. The orchestrator decides what to fix now and sends that back to the same step agent, with a test for each fix.
5. For a UI step the orchestrator looks at the snapshots itself.
6. Green CI on the head commit, then a merge commit. Then the next step.

The orchestrator makes the product and architecture calls that the docs leave open, and has the step record them. It asks the owner only for decisions that are the owner's, and for the checks above.
