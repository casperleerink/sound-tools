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

One `Sequencer` processor per track. Its update, `SequencerUpdate`, is a snapshot or a preview note. The snapshot is an `Arc<TrackSnapshot>`: every note of every clip of the track at its project position, sorted by start and pitch. The track behaviour builds a new one on every run and the processor swaps it in. The old one rides back and is dropped on the control thread. A block finds its notes with two binary searches, whatever the number of clips.

No stuck notes. The processor keeps a fixed list of the notes it started (`HELD_CAPACITY`, 128), each with the tick of its off. Offs come from that list, never from the snapshot. So a note gets its off when its clip was edited, moved or deleted while it sounded, and across a tempo change, because the end is a tick.

- After a swap, each held note looks itself up in the new snapshot by start and pitch. Found: it takes the end it has there. Not found, or that end is already past: its off goes out at once. A note the edit did not touch is found with the same end, so nothing is sent for it. A swap per mouse move during a drag neither cuts nor restarts it.
- On `jumped` or `stopped_playing`: one `AllOff`, and the list is cleared.
- On one tick, offs go before ons.
- A note that would be the 129th held one is not played. An on that does not fit in the event buffer is not played and not listed as held. Each such note counts once in `EngineStatus::event_overflows`, which the runtime prints in `status`, after a render and at the end. An off that does not fit stays in the list and goes out in the next block. After the first event that did not fit, nothing more is pushed in that block, so a waiting off counts at most once per block.

### The preview note

`preview_note(project, track, pitch, velocity)` sounds one note now through the instrument of a track, also while the project is stopped. The note editor calls it when a note is clicked, drawn or moved to a new pitch. It goes through `Project::send` to the sequencer of the track as `SequencerUpdate::Preview`. It is not an edit: nothing is saved and there is no undo step.

The off comes from the sequencer, not from the interface, `PREVIEW_SECONDS` (0.3 s) of engine time later. So no interface can leave a preview sounding: not a fast drag, not a delete, not a closed editor. The rules:

- A new preview ends the one before it, off before on, in the same block. Two previews in one block play the last one.
- A stop or a seek ends it with everything else (`AllOff`).
- An off releases every note of its pitch. So a preview sends no off while a note of the timeline holds its pitch: the off of that note ends both. The other way round, a note of the timeline that ends cuts a preview of its pitch short.
- An off that does not fit in the event buffer waits for the next block. An on that does not fit is dropped and counted.

## Helpers for interfaces

All take the `Project` and a `Changes` group, so an interface puts several in one undo step and publishes them through `Project`.

- `tracks(project, arrangement)`, `clips(project, track)`: in display order.
- `add_track(project, changes, arrangement, name, colour, instrument)`: a track after the last one with its instrument child. The instrument state is a parameter, because this crate knows no instrument: pass `SynthState::default()`. The id comes from the name (`"Warm Pad"` gives `warm-pad`, then `warm-pad-2`).
- `add_clip(project, changes, track, name, clip)`.
- `move_clip(project, changes, clip, to_track)`: a delete and a create in one group, like moving the file.
- `create_default_project(project, instrument)`: the default template. 120 bpm, 4/4, one arrangement `arrangement` with one track and its instrument, no clips.

Ids come from `Project::free_id`, which looks at the project and the disk but not at the group being built. Two tracks of the same name in one group need two groups.

Edit notes with `project.update(&mut edit, &clip, |clip| ...)` on the `Clip` itself. A note editor needs no helper for that.

- `end(project, arrangement)`: the end of the last clip, `None` without clips. It is registered as the end of the tool, so `Project::end` and the transport have a duration.

## The view

`view::register(views)` registers `ArrangementView` for the `arrangement` tool. The guide for views in general is the [sound-ui README](../../crates/ui/README.md). `view.rs` and the files under `view/` are the only modules that use GPUI, and of those `layout`, `gesture` and `roll` are pure math with unit tests.

- `ArrangementView` stacks the timeline over one detail panel. The panel shows one thing at a time (`Detail`): the note editor of a clip or the track panel of a track. Opening one takes the place of the other, and both have one height, so a swap does not move the timeline. Each view is cached, and the timeline and the editor have a playhead line beside them, so that playback repaints the lines only. It opens the editor on `TimelineEvent::OpenEditor` and the track panel on `TimelineEvent::OpenTrack`, gives what is open the clip or the track that gets selected, and closes it on `EditorEvent::Close`, on `TrackPanelEvent::Close`, on escape, and when its clip or its track is deleted, from inside or outside. A drag to another track deletes the clip at its old id. The timeline has selected the new id by then, so the editor follows it.
- The timeline listens to the arrangement, its tracks and their clips only. Another child of a track, such as the instrument, shows nowhere in it, so a knob drag in the track panel reads no clips again and paints no timeline.
- `Timeline` holds the interface state: `Viewport` (zoom and scroll), the selected clip, the selected track, the open drag and a focus handle. A click on a track header selects the track, clears the clip selection and asks for the track panel. The keys go to the selected clip first. With no clip selected, up and down select the track above or below and enter opens its panel. Selecting a clip leaves the track selected, so the header keeps its quiet fill while a clip of the track is edited. Each paint builds a `Scene` from the project: the visible rows, bars and `ClipShape`s, each with its `Instance<Clip>` and its rect. The mouse listeners of that frame get the same scene, and `Scene::zone_at(x, y)` is the hit test: the clip on top with its body or an edge. It keeps the track order and the end of the last clip of each track between project events. An event that names a clip reads only the track of that clip again, so a mouse move of a drag does not walk every clip of the project.
- `NoteEditor` shows one clip as a piano roll on the project timeline, with the same `Viewport` math across and pitch rows of `roll` up. It keeps no scene: it reads the clip when it paints and when a mouse event arrives, with the viewport that was painted. One note is selected, by its value and not by its index: the clip changes under the editor, by an agent, an undo or a clip resize, and an index would then name another note. The note is looked up when a key uses it, and the selection clears when the clip no longer has it.
- `TrackPanel` shows the devices of one track, see "The track panel" below.
- `view::layout`: `Viewport::x_of`, `tick_at`, `y_of`, `track_at`, `nearest_track`, `visible_ticks`, `visible_tracks`, `zoomed`, `scrolled`, `clamped`, `clamped_to`, `clip_rect`, `ruler_bars`, `beat_lines`, `miniature`, and the snap: `SNAP` (a sixteenth), `snap`, `snap_floor`, `snapped_delta`, `shifted`. Its coordinates are those of the timeline area, right of the headers and below the ruler.
- `view::gesture`: `zone_at` (body, left edge, right edge), `new_clip`, `resized_right`, `resized_left`, `nudged_track`.
- `view::roll`: `y_of`, `pitch_at`, `nearest_pitch`, `transposed`, `visible_pitches`, `note_rect`, `note_at`, `opened` (the zoom and scroll of an editor that opens), `clamped`, `drawn_note`, `moved_note`, `resized_note`.

### The track panel

A track is not a synth. It owns clips and one child named `instrument`, and any tool with the ports of the note contract fits that slot. So the panel is about the track, and what it shows of the instrument comes from the extension of that instrument.

- The panel is a rack: device cards from left to right, in the order the sound goes through them. Today a track has one device, the instrument. `track_panel::device_slots` gives the ids of the slots of a track, and today that is `<track>/instrument` alone.
- For each slot the panel asks the view registry for the view of whatever instance is there: `Views::view_of`. It names no instrument type, and this crate still depends on no instrument. The view goes into a plain card. When the slot is empty, or its tool has no registered view, the card says so: `No instrument`, or the tool name with `This tool has no view.`
- The card follows the slot live. When an event names a slot and its tool is not the one the card was made for (another `instrument.json` from outside, a delete, an undo), the panel makes the card again. While the tool stays the same, the device view follows its own record.
- The panel edits nothing itself and keeps no state of the project. Edits are those of the device views, through the session. A knob drag that is open when the panel closes is finished by the device view when it is released.
- The header is that of the note editor: accent dot, track name, close control. The panel has a focus handle that is no tab stop. It only tells `ArrangementView` whether the focus is inside when the panel closes, so that the focus goes back to the timeline. Tab reaches the close control and then the controls of the devices.

How the rack grows. The instrument slot has a fixed name, so its card is made once and made again only when the tool in it changes. Effects will come and go, so they need more than one more id: `device_slots` then reads the effect children from the project, in rack order, and the panel makes its list again on `Created` and `Deleted` inside the track. That rebuild must keep the device of every slot that stays, by its slot id, so that a view with an open knob drag is not dropped for a change next to it. The rendering needs no change, because a card is already made per device, and the rack already scrolls sideways when the cards are wider than the window. The mixer controls of the track (gain, pan, mute) are not devices. They get a fixed section at the right end of the row in `TrackPanel::render`, after the rack, outside of what scrolls. None of this is built, and there are no empty places for it: no effects, sends, instrument picker or reordering.

### Editing rules

Every change goes through the session. A drag is one gesture: `begin_gesture` with the first mouse move that changes something, `gesture` per move, `finish_gesture` on mouse up, `cancel_gesture` on escape. So a plain click is no undo step, sound and every view follow each move, the file is written once, and undo and redo wait until the drag ends. A key is one `commit`. The undo labels are "Add clip", "Move clip", "Resize clip", "Delete clip", "Nudge clip", "Draw note", "Move note", "Resize note", "Delete note" and "Nudge note".

- The snap is a sixteenth, fixed. A drag moves by whole snap steps from where it began and does not snap the result. So a clip or a note that an agent wrote off the grid keeps its offset. A new clip and a drawn note start in the grid cell under the pointer.
- A double click on empty track space adds an empty clip of one bar, named `clip`, `clip-2` and so on.
- A drag of a clip body moves it in time, not before tick 0, and to the track under the pointer. Another track means another id: `move_clip`, a delete and a create in one group. The selection and the open editor go with the clip. A drag that comes back to its first track takes the first id again, so there and back leaves the file where it was. Nothing is written during a drag, so the old file is still there then, and `free_id` would give `part-2`.
- The right edge changes the length. `Clip::set_length` drops the notes that start outside, as the clip rule says. Every move applies to the clip as it was at mouse down, so going in and out again in one drag loses nothing. Undo brings dropped notes back.
- The left edge changes the start and keeps the notes where they are in the project, so their starts change the other way. It drops no note: the edge stops at the first note. It also stops at tick 0 and one snap step before the right edge. This is the simplest correct rule. Cutting a clip inside its notes needs a rule for the cut notes, which comes with splitting clips.
- A clip or a note does not get shorter than one snap step, or than it already was.
- Clips may overlap. There are no collision rules.
- Notes stay inside their clip. A moved note stops where its end meets the clip end, a resized or drawn note ends with the clip at the latest, and a press outside the clip draws nothing. A note that an agent wrote past the clip end stays as it is until it is touched.
- A drawn note has velocity 100. There is no velocity lane yet.
- When a note edit ends, the notes of the clip are put in order by start and pitch, because the agent doc asks that of whoever writes a clip. The selection stays on its note.
- A drag never leaves the gesture of the session open. The view that closes the editor ends its note drag first, `set_clip` does the same, and both views finish an open gesture when they are released, as a net under every other way to go.
- A clip resize keeps the clip of mouse down and what it wrote last. When the live clip is not what it wrote, something else changed it: an undo under a press that did not move yet, or an agent. The resize then goes on from the live clip, and the grab moves by what the drag had done to its edge. So it never writes an old copy with old notes over a newer clip, and a press without a move changes nothing and keeps redo. A move writes only the start, so it keeps what else changed. Before its first move it takes the live start too.
- A move to another track is a delete and a create in one group of events, and so are its undo and redo. The selection goes to the clip of the same name that the same group created, and the editor goes with it. The editor closes only after the group, when its clip is really gone.
- Outside changes apply during a drag and the next mouse move is the later write. When the dragged clip is deleted from outside, the drag ends and the gesture finishes: the delete was the last write, and undo gives the clip back as it was before the drag. A note drag finds its note again by what it wrote last, and ends when the note is gone. Not handled: a file edit of a clip while that clip is dragged across tracks. Its file still has the old id then, so the edit comes back as a second clip.

## Summary

The arrangement registers a summary (`ToolRegistration::summary`), which `runtime <folder> --inspect` prints: each track in order with name, colour, order and instrument tool, and each clip with its id, `bar:beat:tick` range, tick range, note count and pitch range.

## Checks

```sh
cargo nextest run -p arrangement -p runtime
RTSAN_ENABLE=1 cargo nextest run -p arrangement                                   # with the realtime sanitizer
cargo nextest run -p runtime --test window                                       # the views with a simulated mouse and keys
cargo nextest run -p runtime --test window track_panel                           # the track panel and the synth view in it
cargo nextest run -p runtime --run-ignored only hundred_tracks --no-capture      # 100 tracks of 100 clips
cargo test -p runtime --test snapshots                                           # the window as PNGs, with frame times
```

Measured September 19, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline: a project of 100 tracks with 100 clips each (10,200 records, an eighth of the tracks playing a chord at any time) opens in 0.44 s, applies one outside clip edit in 0.5 ms, and plays 62 times faster than realtime.
