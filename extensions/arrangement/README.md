# arrangement

The bundled arrangement extension: tracks, clips and notes on the project timeline. This file is for extension and interface authors. The record formats, with a complete example of each, are in [agent-doc.md](agent-doc.md), which the runtime also writes into every project as part of `AGENTS.md`. That file is the single source for the formats; a test loads every example in it.

Enable it in `project.json` under `extensions` as `"arrangement"`.

## Tools and state types

| Tool | State | Form | Behaviour |
| --- | --- | --- | --- |
| `arrangement` | `ArrangementState`, no fields yet | `<name>/instance.json` | none. It owns the tracks and gives the summary. |
| `arrangement.track` | `TrackState`: `name`, `colour`, `order` | `<name>/instance.json` | one `Sequencer`, routed to the child `instrument` and to device channels 0 and 1 |
| `arrangement.clip` | `sound_notes::Clip`: `start`, `length`, `notes` | `<name>.json` | none, plain data for its track |

`Clip` lives in the contract crate `crates/notes`, because its saved form is what other extensions read. This crate does not depend on any instrument. A track finds its instrument by the child name `instrument` (`INSTRUMENT`) and the port names `NOTES_INPUT` and `AUDIO_OUTPUT`, so any tool with those ports fits. A track without that child loads and is silent.

Each tool says where it lives (`State::PLACE`): the arrangement at the top of `state/`, a track in an arrangement, a clip in a track. A record anywhere else is not loaded and the problem says where it belongs, so an agent that forgot the track folder gets a signal and not silence.

`Colour` is an enum of the accent names of DESIGN.md, saved in lowercase. It is not a hex string, so a record cannot hold a colour the design has no token for. Map it to a token in the interface with `Colour::name()`.

## Rules

- Note starts count from the start of their clip. Every note starts inside the clip: `start` below the clip `length`. A record that breaks this does not load, and the message names the note. So a note written with a project position is an error an agent sees, not silence. `Clip::set_length` drops the notes a shorter clip cannot hold, for good. So a resize drag applies every move to the clip as it was when the gesture began, not to the live clip. Else dragging in and out again loses the notes in between.
- A note that is longer than the rest of its clip ends where the clip ends.
- Clips on one track may overlap. The notes of all of them play.
- Two sounding notes of one pitch on one track sound until the last of them ends. In the note contract an `Off` releases every held note of its pitch, so the sequencer sends the one off with the last holder. The result is the same in any order of ends and with or without a snapshot swap in between.
- Tracks show by `order`, then by id. `tracks()` gives them in that order. `clips()` gives the clips of a track by start, then by id.
- A seek or a stop silences what sounds. A note is never started in its middle: there is no chase.

## The sequencer

One `Sequencer` processor per track. Its update is an `Arc<TrackSnapshot>`: every note of every clip of the track at its project position, sorted by start and pitch. The track behaviour builds a new one on every run and the processor swaps it in. The old one rides back and is dropped on the control thread. A block finds its notes with two binary searches, whatever the number of clips.

No stuck notes. The processor keeps a fixed list of the notes it started (`HELD_CAPACITY`, 128), each with the tick of its off. Offs come from that list, never from the snapshot. So a note gets its off when its clip was edited, moved or deleted while it sounded, and across a tempo change, because the end is a tick.

- After a swap, each held note looks itself up in the new snapshot by start and pitch. Found: it takes the end it has there. Not found, or that end is already past: its off goes out at once. A note the edit did not touch is found with the same end, so nothing is sent for it. A swap per mouse move during a drag neither cuts nor restarts it.
- On `jumped` or `stopped_playing`: one `AllOff`, and the list is cleared.
- On one tick, offs go before ons.
- A note that would be the 129th held one is not played. An on that does not fit in the event buffer is not played and not listed as held. Each such note counts once in `EngineStatus::event_overflows`, which the runtime prints in `status`, after a render and at the end. An off that does not fit stays in the list and goes out in the next block. After the first event that did not fit, nothing more is pushed in that block, so a waiting off counts at most once per block.

## Helpers for interfaces

All take the `Project` and a `Changes` group, so an interface puts several in one undo step and publishes them through `Project`.

- `tracks(project, arrangement)`, `clips(project, track)`: in display order.
- `add_track(project, changes, arrangement, name, colour, instrument)`: a track after the last one with its instrument child. The instrument state is a parameter, because this crate knows no instrument: pass `SynthState::default()`. The id comes from the name (`"Warm Pad"` gives `warm-pad`, then `warm-pad-2`).
- `add_clip(project, changes, track, name, clip)`.
- `move_clip(project, changes, clip, to_track)`: a delete and a create in one group, like moving the file.
- `create_default_project(project, instrument)`: the default template. 120 bpm, 4/4, one arrangement `arrangement` with one track and its instrument, no clips.

Ids come from `Project::free_id`, which looks at the project and the disk but not at the group being built. Two tracks of the same name in one group need two groups.

Edit notes with `project.update(&mut edit, &clip, |clip| ...)` on the `Clip` itself. A note editor needs no helper for that.

## Summary

The arrangement registers a summary (`ToolRegistration::summary`), which `runtime <folder> --inspect` prints: each track in order with name, colour, order and instrument tool, and each clip with its id, `bar:beat:tick` range, tick range, note count and pitch range.

## Checks

```sh
cargo nextest run -p arrangement -p runtime
RTSAN_ENABLE=1 cargo nextest run -p arrangement                                   # with the realtime sanitizer
cargo nextest run -p runtime --run-ignored only hundred_tracks --no-capture      # 100 tracks of 100 clips
```

Measured September 19, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline: a project of 100 tracks with 100 clips each (10,200 records, an eighth of the tracks playing a chord at any time) opens in 0.44 s, applies one outside clip edit in 0.5 ms, and plays 62 times faster than realtime.
