# Milestone 2: plugins, MIDI recording and fit tempo

Decided September 20, 2026. This is the plan an orchestrating agent works from. It holds goals, decisions and checks, no implementation. [ARCHITECTURE.md](../ARCHITECTURE.md) stays the source of truth. Each step records what it settles there.

## Goal

A composer records a piano take from a MIDI keyboard, with no click, on a track that plays a third-party plugin. One action fits the project tempo to the take. The grid now follows the playing, and the take sounds the same as before. A steadiness amount moves the tempo between as played and steady. Parts added after that, by hand or by an external agent, follow the take.

Play freely, and the grid comes to you. That is the first reason to use Sound Tools over another DAW.

## Decided

Agent:

- The agent stays external. It is Claude Code or Codex, run in the project folder. The in-app agent, the sidebar and the outer application are parked. The project menu gets a way to open a terminal in the project folder.
- `AGENTS.md` becomes a short map. Detail lives in one doc per extension or task, and an agent reads a doc when it needs it. A test still loads every example in those docs.

Recording:

- MIDI recording only. Audio recording is a later milestone.
- Recording works with and without a click. The sustain pedal is recorded and played.
- The raw take is kept in the project in real time. No edit changes it.

Plugins:

- CLAP and VST3, as instruments and as effects on a track. CLAP first, then VST3 on the same host design. AU comes later.
- The VST3 SDK is MIT licensed since 3.8. When we write "VST" we follow Steinberg's trademark rules.
- Plugins run in the app's process. A plugin that crashes takes the app down. We accept that for this milestone. A plugin that crashes while we scan it must not.
- Plugin state is opaque. The project saves it as an asset, agents cannot edit it, and a change made in a plugin's own window is not an undo step.
- A missing plugin leaves its record untouched. The project reports it as a problem and the track is silent.

Tracks:

- The signal path becomes stereo.
- A track gets gain, pan and mute. They are saved in the track record and shown at the right end of the track panel.

Fit tempo:

- A deterministic algorithm finds the beats, not an LLM. The agent's part is what the algorithm cannot know. It corrects the fit by editing files: the time signature, the first downbeat, half or double tempo.
- The fitted tempo is one step per beat. It belongs to the whole project, and other parts follow the new grid.
- At 0 % steadiness the tempo follows the take, at 100 % it is one tempo. Notes keep their ticks, so the touch inside each beat stays. It can always be turned back. One amount per project for now.

Rules that stay:

- The core knows no notes, MIDI or plugins. All of this is extensions on the SDK, and extensions do not depend on each other.
- CI needs no third-party plugin and no MIDI device.
- Every edit goes through the one editing path, with undo. Outside file edits keep applying live.

## Out of scope

Audio recording, the in-app agent and the outer application, AU, a plugin sandbox, latency compensation, automation of plugin parameters, solo, sends and buses, loop playback, tempo ramps, time signature changes, overdub and merging takes, steadiness per section, collaboration.

## Steps, one pull request each

| # | Step | Done when |
| --- | --- | --- |
| 0 | Agent docs as a map, and a terminal from the project menu | A fresh outside agent adds a part and a track as in milestone 1, reading only the docs it needs. |
| 1 | Stereo signal path and track gain, pan and mute | A track plays in stereo. Gain, pan and mute work from the window and from a file, live, each as one undo step. Old projects still open. |
| 2 | Recording basics: metronome, tempo shown and editable, the view follows the playhead | The click lands on the beats of the tempo map. A tempo edit is one undo step. |
| 3 | MIDI input and recording | Playing a keyboard sounds through the track's instrument with low latency. A take becomes a clip, with the pedal, as one undo step. The raw take is in the project. |
| 4 | Project assets and CLAP instruments | A composer picks a CLAP instrument for a track, opens its window, and hears it. Its state survives close and reopen. A missing plugin is a reported problem. |
| 5 | VST3 instruments | The same, for VST3. |
| 6 | Effect plugins in the track rack | Effects sit after the instrument in the track panel, in order, and the sound passes through them. |
| 7 | Fit tempo and steadiness | See the milestone checks below. |
| 8 | Milestone check | Every check below has evidence, the README is current, and the known gaps are listed. |

Steps 0 to 2 are small. Steps 4 and 5 are the largest and may each split in two.

## Verify the milestone

- A take recorded with no click, then fitted: at 0 % steadiness it renders the same as the raw take, within one tick per note. At 100 % the beats are evenly spaced and each note keeps its place inside its beat.
- On generated takes with a known tempo curve, the fitted beats land close to the true beats. The step states the bound it reaches.
- An outside agent corrects a wrong fit by editing files, for example double tempo to normal, and the window follows.
- After the fit, an outside agent adds a bass line under the take. It follows the rubato in the window and in an offline render.
- A plugin instrument of each format plays a recorded take. Close and reopen restores the plugin, its sound and the take.
- A project that names a plugin this machine does not have opens, reports it, and leaves the record as it was.
- Recording adds no xruns. The step measures and reports the latency from key press to the instrument.
- The realtime checks pass on every `process` function of our own code.
- Everything from milestone 1 still passes.

## What the owner checks

No agent can hear or play. The orchestrator asks the owner for these and goes on with other work meanwhile:

- After step 3: play a keyboard live. Does the latency feel right?
- After step 5: load the piano plugins you use. Do they sound right, and do their windows work?
- After step 7: record a real free take and fit it. Is the grid where you hear the beats?

## How to run it

The same way as milestone 1:

1. The orchestrator creates a branch from `main` for the step and starts one step agent with [the shared brief](agent-brief.md) and a step brief: context, goal, decided constraints, deliverables, how to verify. No implementation.
2. The step agent builds, runs every check, opens the pull request and waits for green CI.
3. The orchestrator starts a second agent with no shared context as a reviewer. It reviews only, tries to reproduce what it finds, and ranks findings as blocker, should or later.
4. The orchestrator decides what to fix now and sends that back to the same step agent, with a test for each fix.
5. For a UI step the orchestrator looks at the snapshots itself.
6. Green CI on the head commit, then a merge commit. Then the next step.

The orchestrator makes the product and architecture calls that the docs leave open, and has the step record them. It asks the owner only for decisions that are his, and for the checks above.
