# Milestone 4: audio tracks, recording, a sampler and drums

Drafted September 26, 2026. This is the plan an orchestrating agent works from. It holds goals, decisions and checks, no implementation. [ARCHITECTURE.md](../ARCHITECTURE.md) stays the source of truth. Each step records what it settles there. The starting point is "Known gaps after the third milestone" in the same file.

## Goal

A composer brings sound into a piece, not only notes. They drop audio files into the arrangement, trim, move and fade them, and record a voice or a guitar onto a track in time with everything else. They play a sample across the keyboard, and build a beat on a drum pad that sounds good with no sample files at all. An outside agent places audio clips and writes drum patterns by file, as it does with notes today.

## Decided

Media:

- Audio files live in the project folder under `assets/audio/`, next to the takes and plugin states that already use `assets/`. A later kind of media, such as video, gets its own folder there. Importing a file copies it in, so a project folder is complete on its own and plays the same on another machine.
- A record names a media file by its asset name, like `state_asset` today, never by a path outside the project.
- Audio files are never changed. Every edit is to a record: where a clip sits, which part of the file it plays, its gain and its fades.
- WAV and AIFF at least, at any sample rate and bit depth. A file at another sample rate than the device plays at the right pitch and speed.
- Nothing derived from a file, such as a waveform overview, is written into the project folder.
- Reading media lives in a crate that is no extension, like `crates/notes`, so audio clips, the sampler and the drum pad share it and no extension depends on another.

Audio tracks:

- A track is an instrument track or an audio track, chosen when it is made. An audio track has clips of audio instead of notes and no instrument. Its effects, volume, pan, mute, solo and meter are the same as on any track.
- An audio clip is placed at a musical position and plays at its own speed. A tempo change moves where it starts, not how fast it plays. No time-stretching.
- Dragging a file from the Finder onto the arrangement makes an audio clip. The editing of the third milestone works on audio clips too: select, move, trim, copy, paste, duplicate, delete, undo.
- Clip edges never click.

Recording audio:

- Record from the system's default input device, as set in macOS. A track chooses its input channel or channels. No device picker in the window.
- A take is a new file in `assets/audio/` and a clip where the composer heard it, with latency compensation and the input latency taken into account.
- No input monitoring in software: the composer hears the source directly or through their interface.

Instruments:

- A Sampler: one sample played across the keyboard, with a root note, start and end, an envelope and velocity to volume. Dragging a file onto its card loads it.
- A Drum pad: 16 pads, each a sample or a synthesized drum sound, with volume, pitch, decay and pan per pad and one choke group for hi-hats. Pads answer to notes 36 to 51, so a MIDI keyboard and the note editor reach them as in other DAWs.
- A default drum kit made by synthesis inside the Drum pad: kick, snare, clap, closed and open hat, toms and a rim at least. No sample files ship with the product.
- Both are bundled extensions on the pattern of the built-in effects: own look, a picture of what they do, parameters in the project, an agent doc. Large sampled instruments stay the job of VST and CLAP plugins.

Rules that stay:

- The core knows no notes, MIDI, plugins, effects or audio files. All of this is extensions on the SDK, and extensions do not depend on each other.
- CI needs no third-party plugin, no MIDI device, no audio device and no display.
- Every edit goes through the one editing path, with undo. Outside file edits keep applying live.
- The realtime checks pass on every `process` function of our own code. Nothing reads a file on the audio thread.

## Out of scope

Time-stretching and warping, comping and take lanes, crossfades between clips, software input monitoring, a device picker, editing the audio of a file, MP3 and AAC, video, shipped sample packs or a sample browser, multi-sample zones and slicing in the Sampler, the in-app agent, automation, sends and buses, sidechain, loop playback.

## Steps, one pull request each

| # | Step | Done when |
| --- | --- | --- |
| 0 | Design | Mockups in the approved direction for an audio clip with its waveform, an audio track while recording, the Sampler card and the Drum pad card. The owner has approved them. |
| 1 | Media and audio clips | Files import into `assets/audio/`, audio tracks play their clips in time with the rest, and the arrangement edits them as it edits note clips. A missing file is a problem, not a crash. |
| 2 | Recording audio | A take from the default input lands where it was heard, as a file and a clip, with one undo step. |
| 3 | Sampler | See "Verify the milestone". |
| 4 | Drum pad and the default kit | See "Verify the milestone". |
| 5 | Milestone check | Every check below has evidence, the README is current, and the known gaps are listed. |

Step 0 goes first. Step 1 follows, because every later step reads media. Steps 2, 3 and 4 may run in parallel once step 1 is merged. Steps 1 and 4 may each split in two.

## Verify the milestone

- An imported file plays and renders sample-accurate at its position, at the right pitch and length also when its sample rate differs from the device. Trim, gain and fades measure as set.
- A project folder copied to another place opens and renders byte for byte the same. A file missing from `assets/audio/` is listed in `problems.txt` and the rest of the project plays.
- A recorded take lands where it was heard, within a stated bound, shown with a simulated input in CI and with a real input by hand.
- Sampler: each key plays the sample at the right pitch, measured, and its envelope measures as set.
- Drum pad: each pad sounds from its note, the choke group cuts the open hat, and the default kit sounds in a new project with no sample files.
- Every new instrument restores its parameters after close and reopen, and an outside agent changes one by file while it plays.
- An outside agent adds an audio clip from a file in `assets/audio/` and a drum pattern on a Drum pad track, reading only the docs it needs.
- A project of 16 audio tracks of 10 minutes each plays on the laptop with no dropouts, with memory within a stated bound.
- Everything from milestones 1 to 3 still passes.

## What the owner checks

- After step 0: approve the mockups, or change them.
- After step 1: drop a few of your own files in, trim, move and fade them. Does it feel right?
- After step 2: record a voice or an instrument. Does the take land in time?
- After step 3: play the Sampler from a keyboard with a sample of your own.
- After step 4: build a beat with the default kit. Does it sound good enough to keep?
- After step 5: make a short piece with audio, a sampler and drums only in the window.

## How to run it

The same way as milestone 3: one branch and one step agent per step with [the shared brief](agent-brief.md), an independent reviewer per pull request, fixes back to the same step agent with a test each, green CI on the head commit, then a merge commit. Parallel steps each use their own Cargo target directory. The orchestrator makes the product and architecture calls the docs leave open and has the step record them. It asks the owner only for decisions that are the owner's, and for the checks above.
