# arrangement

The bundled arrangement extension: tracks, clips and notes on the project timeline. This file is for extension and interface authors. The record formats, with a complete example of each, are in [agent-doc.md](agent-doc.md), which the runtime also writes into every project as `agent-docs/arrangement.md`, listed in the map `AGENTS.md`. That file is the single source for the formats; a test loads every example in it.

Enable it in `project.json` under `extensions` as `"arrangement"`.

## Tools and state types

| Tool | State | Form | Behaviour |
| --- | --- | --- | --- |
| `arrangement` | `ArrangementState`: `master` (`MasterState`: `gain_db`, `limiter`) | `<name>/instance.json` | one `Mixer` per track and one `Master`: every track, its mixer, the master, main output. It owns the tracks and gives the summary. |
| `arrangement.track` | `TrackState`: `name`, `colour`, `order`, `gain_db`, `pan`, `mute`, `solo`, `effects` | `<name>/instance.json` | one `Sequencer`: sequencer, child `instrument`, the effects in order that are not bypassed, and out as its `audio` output |
| `arrangement.clip` | `sound_notes::Clip`: `start`, `length`, `notes` | `<name>.json` | none, plain data for its track |

`Clip` lives in the contract crate `crates/notes`, because its saved form is what other extensions read. This crate does not depend on any instrument and on no effect. A track finds its instrument by the child name `instrument` (`INSTRUMENT`) and the port names `NOTES_INPUT` and `AUDIO_OUTPUT`, and each of its effects by the name the record lists and the port names `AUDIO_INPUT` and `AUDIO_OUTPUT`, so any tool with those ports fits either place. A track without an instrument loads and is silent.

Each tool says where it lives (`State::PLACE`): the arrangement at the top of `state/`, a track in an arrangement, a clip in a track. A record anywhere else is not loaded and the problem says where it belongs, so an agent that forgot the track folder gets a signal and not silence.

`Colour` is an enum of the accent names of DESIGN.md, saved in lowercase. It is not a hex string, so a record cannot hold a colour the design has no token for. Map it to a token in the interface with `Colour::name()`.

`gain_db` (a number up to 6, `TrackState::MAX_GAIN_DB`, or `"-inf"`), `pan` (-1 to 1, `TrackState::PAN`), `mute` and `solo` are the mixer of the track. A record that leaves them out plays as it did before they existed: no change of level, in the middle, not muted, not soloed. A value outside its range does not load and the problem names the field, like any other record value. The ranges are written once, in `TrackState`, and `validate`, the controls of the track panel and the docs read them there.

A gain in decibels is saved as a number, or as the string `"-inf"` for silence, which JSON has no number for (`decibels`). So the bottom of a volume is truly silent, and a record of before this reads and writes as it did. The master volume follows the same rule.

## Rules

- Note starts count from the start of their clip. Every note starts inside the clip: `start` below the clip `length`. A record that breaks this does not load, and the message names the note. So a note written with a project position is an error an agent sees, not silence. `Clip::set_length` drops the notes a shorter clip cannot hold, for good. So a resize drag applies every move to the clip as it was when the gesture began, not to the live clip. Else dragging in and out again loses the notes in between.
- A note that is longer than the rest of its clip ends where the clip ends.
- Clips on one track may overlap. The notes of all of them play.
- Two sounding notes of one pitch on one track sound until the last of them ends. In the note contract an `Off` releases every held note of its pitch, so the sequencer sends the one off with the last holder. The result is the same in any order of ends and with or without a snapshot swap in between.
- Tracks show by `order`, then by id. `tracks()` gives them in that order. `clips()` gives the clips of a track by start, then by id.
- A seek or a stop silences what sounds. A note is never started in its middle: there is no chase.
- A clip may hold the sustain pedal, which a recording writes. While the pedal is down a note goes on sounding after its own end, until the pedal comes up. The pedal of a clip ends with the clip, as a note that is longer than the rest of its clip ends there, so a clip that ends under the pedal does not sustain for the rest of the piece. Pedal moves of clips that overlap all play, in tick order, and the last one wins, including a lift at the end of a clip.
- A clip may also name the raw take it was recorded from (`take`). The arrangement keeps the field through every edit and never reads it; see `extensions/midi`.

## The sequencer

One `Sequencer` processor per track. Its update, `SequencerUpdate`, is a snapshot or a preview note. The snapshot is an `Arc<TrackSnapshot>`: every note of every clip of the track at its project position, sorted by start and pitch. The track behaviour builds a new one on every run and the processor swaps it in. The old one rides back and is dropped on the control thread. A block finds its notes with two binary searches, whatever the number of clips.

No stuck notes. The processor keeps a fixed list of the notes it started (`HELD_CAPACITY`, 128), each with the tick of its off. Offs come from that list, never from the snapshot. So a note gets its off when its clip was edited, moved or deleted while it sounded, and across a tempo change, because the end is a tick.

- After a swap, each held note looks itself up in the new snapshot by start and pitch. Found: it takes the end it has there. Not found, or that end is already past: its off goes out at once. A note the edit did not touch is found with the same end, so nothing is sent for it. A swap per mouse move during a drag neither cuts nor restarts it.
- On `jumped` or `stopped_playing`: one `AllOff`, and the list is cleared.
- On one tick, offs go before ons.
- A note that would be the 129th held one is not played. An on that does not fit in the event buffer is not played and not listed as held. Each such note counts once in `EngineStatus::event_overflows`, which the runtime prints in `status`, after a render and at the end. An off that does not fit stays in the list and goes out in the next block. After the first event that did not fit, nothing more is pushed in that block, so a waiting off counts at most once per block.

The pedal has no list of its own, because it is one value. Every block, while the project plays, the sequencer compares the pedal the snapshot has just before the block with the last value it sent, and sends the difference at offset 0, before the notes. Then it sends the moves inside the block. So a seek into a held pedal arrives with the pedal down, an edit that removes a pedal releases what it held, and a stop or a seek puts it up with the `AllOff`. There is no case of its own for any of them.

The snapshot does two things to the pedal while it builds. It lifts it at the end of every clip whose pedal is down there, so the pedal of a clip ends with the clip. And it keeps one move per tick, the most pressed of them: a record may hold any number on one tick, and the work of a block is then bounded by the ticks it covers.

### The preview note

`preview_note(project, track, pitch, velocity)` sounds one note now through the instrument of a track, also while the project is stopped. The note editor calls it when a note is clicked, drawn or moved to a new pitch. It goes through `Project::send` to the sequencer of the track as `SequencerUpdate::Preview`. It is not an edit: nothing is saved and there is no undo step.

The off comes from the sequencer, not from the interface, `PREVIEW_SECONDS` (0.3 s) of engine time later. So no interface can leave a preview sounding: not a fast drag, not a delete, not a closed editor. The rules:

- A new preview ends the one before it, off before on, in the same block. Two previews in one block play the last one.
- A stop or a seek ends it with everything else (`AllOff`).
- An off releases every note of its pitch. So a preview sends no off while a note of the timeline holds its pitch: the off of that note ends both. The other way round, a note of the timeline that ends cuts a preview of its pitch short.
- An off that does not fit in the event buffer waits for the next block. An on that does not fit is dropped and counted.

## The effects of a track

The chain of a track is its instrument, then its effects in the order `TrackState::effects`
names them, then its mixer, then the main output. One place decides the order, so a reorder is
one record and one undo step, and the chain a file says is the chain that plays.

- An effect is a child record of the track with an `audio` input and an `audio` output, under
  any name but `instrument`. The list holds the child name, so a name in the list is a file
  next to the track record. Two lines make one effect, and each half without the other is a
  reported problem that says what to write.
- What the record refuses, because there would be no order to read: a name that is not a child
  name, a name twice, and `instrument`. The track then keeps what it had, as for any record
  that does not load.
- A record that leaves `effects` out has no effects and is written back without it, so a track
  of before this existed loads unchanged and gives the same bytes.
- A listed name whose record is missing, or whose tool has no audio ports, is left out of the
  chain and reported: the sound goes through the rest, so one missing plugin never silences a
  track. A child that looks like an effect and is in no list is reported too, because nothing
  goes through it.
- `device_slots(project, track)` gives the slots of a track in rack order, which is what the
  track panel draws. `add_effect(project, changes, track, name)` gives a free slot id and
  appends its name to the record; the caller puts a record in that id in the same group, as
  `add_track` takes an instrument, so adding an effect is one undo step. `remove_effect` takes
  the record and the name out together, so undo brings it back where it was.
- `add_effect` reads the record of the project and not the group being built, like `free_id`.
  Two effects in one group need two groups.
- `move_effect(project, changes, track, slot, to)` moves a slot to place `to` among the effects,
  0 right after the instrument; a place past the last is the last. Only the list changes, so
  the slot keeps its record and its bypass. It gives whether anything moved.

The tail of an effect. An effect is a processor like any other: the engine runs it every block,
whether the project plays or not, so a delay or a reverb rings out after a stop. Taking an
effect off the track takes its processor out of the graph, so its tail goes with it at once,
which is the hard switch every routing edit is. A missing plugin is the same: the slot passes
its input through and what the plugin held is gone.

## The mixer

One `Mixer` processor per track, after the instrument and every effect, and before the master.
The arrangement owns them and not the tracks, because solo is about every track at once. Audio is
stereo everywhere (`sound_core::CHANNELS`), so the mixer is two gains, one per channel.

- `channel_gains(track)` turns the record into those two gains on the control thread. The audio thread works out no pan law. The behaviour of the arrangement sends them on every run, which costs no compile.
- Solo: while any track is soloed, the arrangement sends every other track the gains of a muted one, `[0, 0]`. So soloing a track sounds sample for sample as muting every other one. A muted track stays silent when it is soloed.
- The pan law is equal power, scaled so that the middle is exactly 1 in both channels. A centred track at 0 dB is untouched, sample for sample, so a project from before the mixer sounds the same. Hard left or right, the channel that plays it is √2, 3 dB above the middle, and the other is exactly 0. The power of a track is the same wherever it is panned.
- The processor ramps to a new pair over `RAMP_SECONDS` (20 ms), so no change of gain, pan, mute or solo clicks. The largest step it can take in one frame is the distance divided by the ramp. A muted track that has finished its fade returns before it touches its output.
- A change of the mixer keeps everything else: a note goes on sounding through it, because the behaviours keep every processor and only send an update.
- The mixer keeps the peaks of what it sends on, which is the meter of the track: `track_peaks(project, track)`. They are after the volume, the pan, mute and solo.

A track sends the end of its chain up as its `audio` output, and the arrangement connects that to
the mixer of the track. A bypassed effect is left out of the chain: the sound goes past it
untouched, and its latency goes with it, see "The effects of a track".

## The master

The arrangement record holds the master: `master.gain_db`, its volume, and `master.limiter`. A record
that leaves them out, which is every record of before this, gets 0 dB and the limiter on at its
defaults. One `Master` processor plays it: the sum of every mixer, the volume, then the limiter,
then the main output.

- The volume is before the limiter, so no volume can push the output over the ceiling. It ramps like the mixer of a track.
- The limiter: `gain_db` (0 to 24) into it, `ceiling_db` (-24 to 0, default 0, full scale), `release_ms` (10 to 1000, default 100) and `lookahead_ms` (0 to 10, default 0). The ranges are written once in `LimiterState`.
- The gain of the limiter goes down at once to what a peak needs and comes back along the release, a time constant. So no sample goes over the ceiling, and a last clamp at the ceiling catches rounding. Under the ceiling the gain is exactly 1: the output is the input, sample for sample.
- With a lookahead the limiter holds the sound back by that time and lowers the gain along a straight line over it, so the gain is down when the peak arrives and the top of the wave keeps its shape. It says the lookahead as its latency (`Processor::latency`), so every track is led by it and reaches the device in time. The default is no lookahead, because a lookahead delays a keyboard played live and a preview note as much.
- `bypass` lets the sound through untouched and keeps the latency as a pure delay, so switching the limiter off and on never moves the tracks in time. The gain goes on working while it is bypassed, so switching it on again needs no clip.
- With no lookahead the rising edge of the first peak over the ceiling is flattened at the ceiling: a hard clip of that edge, and the release then turns the next peaks down whole. With a lookahead, for up to one lookahead after the ceiling is lowered during playback, the last clamp flattens what was planned for the old ceiling.
- After a peak the gain comes back to exactly 1: it works in f64 and goes the rest of the way within 0.001 dB. Samples that are not a number or infinite come out as 0.
- The master keeps the peaks of what it sends out, `master_peaks(project, arrangement)`, and of how much the limiter took, `reduction_peaks`, as the factor by which the sound was above the output.
- Only what the arrangement plays goes through the master. A `project.json` connection to the device and the click of the metronome go around it.

## Helpers for interfaces

All take the `Project` and a `Changes` group, so an interface puts several in one undo step and publishes them through `Project`.

- `tracks(project, arrangement)`, `clips(project, track)`: in display order.
- `add_track(project, changes, arrangement, name, colour, instrument)`: a track after the last one with its instrument child. The instrument state is a parameter, because this crate knows no instrument: pass `SynthState::default()`. The id comes from the name (`"Warm Pad"` gives `warm-pad`, then `warm-pad-2`).
- `add_clip(project, changes, track, name, clip)`.
- `add_clips(project, changes, clips)`: several clips of `(track, name, clip)` in one group, for a paste or a duplicate. No two get the same id. A name loses a number at its end, then takes the next free one: a copy of `verse-2` is `verse` when that is free, else `verse-2`, `verse-3` and so on. The disk is read once per clip, not once per number taken.
- `move_clip(project, changes, clip, to_track)`: a delete and a create in one group, like moving the file.
- `create_default_project(project, instrument)`: the default template. 120 bpm, 4/4, one arrangement `arrangement` with one track and its instrument, no clips.

Ids come from `Project::free_id`, which looks at the project and the disk but not at the group being built. Two tracks of the same name in one group need two groups.

Edit notes with `project.update(&mut edit, &clip, |clip| ...)` on the `Clip` itself. A note editor needs no helper for that.

- `end(project, arrangement)`: the end of the last clip, `None` without clips. It is registered as the end of the tool, so `Project::end` and the transport have a duration.

## The view

`view::register(views)` registers `ArrangementView` for the `arrangement` tool. The guide for views in general is the [sound-ui README](../../crates/ui/README.md). `view.rs` and the files under `view/` are the only modules that use GPUI, and of those `layout`, `gesture` and `roll` are pure math with unit tests.

- `ArrangementView` stacks the timeline over one detail panel. The panel shows one thing at a time (`Detail`): the note editor of a clip, the track panel of a track, or the master panel. Opening one takes the place of the other. The note editor is 352 pt and the two panels 216 pt, so a swap between the editor and a panel moves the lower edge of the timeline. Under the timeline, above the panel, is the master row. Each view is cached, and the timeline and the editor have a playhead line beside them, so that playback repaints the lines only. It opens the editor on `TimelineEvent::OpenEditor` and the track panel on `TimelineEvent::OpenTrack`, gives what is open the clip or the track that gets selected, and closes it on `EditorEvent::Close`, on `TrackPanelEvent::Close`, on escape, and when its clip or its track is deleted, from inside or outside. A drag to another track deletes the clip at its old id. The timeline has selected the new id by then, so the editor follows it.
- The timeline listens to the arrangement, its tracks and their clips only. Another child of a track, such as the instrument, shows nowhere in it, so a knob drag in the track panel reads no clips again and paints no timeline.
- `Timeline` (`view/timeline.rs`) holds the interface state: `Viewport` (zoom and scroll), the selected clips (`view::selection::Selection`, the first of them is what the note editor shows), the selected track, the selected tempo change, the clipboard, the snap setting, the name field of a track while it is open, the open drag and a focus handle. A click on a track header selects the track, clears the clip selection and asks for the track panel. The keys go to the selected clips first. With no clip selected, up and down select the track above or below, enter edits its name and cmd-down opens its panel. The keys are the timeline's only while it has the focus itself, not a control inside it such as the snap setting or the name field. Selecting a clip leaves the track selected, so the header keeps its quiet fill while a clip of the track is edited. Each paint builds a `Scene` from the project: the visible rows, bars and `ClipShape`s, each with its `Instance<Clip>` and its rect. The mouse listeners of that frame get the same scene, and `Scene::zone_at(x, y)` is the hit test: the clip on top with its body or an edge. It keeps the track order and the end of the last clip of each track between project events. An event that names a clip reads only the track of that clip again, so a mouse move of a drag does not walk every clip of the project.
- `NoteEditor` shows one clip as a piano roll on the project timeline, with the same `Viewport` math across and pitch rows of `roll` up, and the velocity lane in its lowest 56 pt. It keeps no scene: it reads the clip when it paints and when a mouse event arrives, with the viewport that was painted. The selected notes are a `Selection<Note>`, by value and not by index: the clip changes under the editor, by an agent, an undo or a clip resize, and an index would then name another note. A note is looked up when a key uses it, and leaves the selection when the clip no longer has it. An undo or a redo selects the notes it brought (`Session::history_moves`).
- The timeline follows the playhead. While the project plays it pages forward when the playhead passes the right edge, and the playhead lands back at the left edge. A jump, which is a seek or a stop, brings it back into view the same way. While the composer has scrolled the playhead off screen nothing pulls the view back, until the next jump: `set_viewport` decides from where the playhead lands whether the view keeps following. The rule itself is `Viewport::shows` and `Viewport::following`, both pure. The follow runs on every playhead change and notifies only when the view really moves, so the timeline is still not painted per frame. The note editor does not follow: it shows one clip.
- The scroll room reaches at least to the playhead, so the view can follow past the end of the piece.
- `TrackPanel` shows the devices of one track, see "The track panel" below.
- `view::layout`: `Viewport::x_of`, `tick_at`, `y_of`, `track_at`, `nearest_track`, `visible_ticks`, `visible_tracks`, `shows`, `following`, `zoomed`, `scrolled`, `clamped`, `clamped_to`, `clip_rect`, `ruler_bars`, `beat_lines`, `miniature`, `shifted`, and `rows_between`, the rows a rectangle touches. Its coordinates are those of the timeline area, right of the headers and below the ruler.
- `view::snap`: the setting `Snap` (off, bar, beat, 1/8, 1/16, 1/32), `SharedSnap`, the one the timeline and the note editor share, and `Grid` with its `step` (what a drag moves by and a new shape starts on, one tick when off) and its `unit` (what an arrow moves by, the length of a new note and the shortest a drag makes a shape, a sixteenth when off). `Grid::free` is the grid with cmd held. `snap`, `snap_floor` and `snapped_delta` take a step.
- `view::selection`: `Selection<T>`, a set with one first thing. `toggle` for shift-click and cmd-click, `set`, `remove`.
- `view::clipboard`: `CopiedClips`, clips with their distances in time and rows, and where a paste puts them (`placed`); `CopiedNotes`, the same for notes in a clip; `Copied`, one of them; and `SharedClipboard`, the one clipboard of the window that the timeline and the note editor share.
- `view::gesture`: `zone_at` (body, left edge, right edge), `new_clip`, `resized_right`, `resized_left`, `nudged_track`.
- `view::roll`: `y_of`, `pitch_at`, `nearest_pitch`, `transposed`, `visible_pitches`, `note_rect`, `note_at`, `notes_in` (what a rectangle touches), `opened` (the zoom and scroll of an editor that opens), `clamped`, `drawn_note`, `moved_note`, `moved_notes` (several as a whole), `resized_note`, and of the velocity lane `velocity_y`, `velocity_at`, `moved_velocity`, `velocity_bar`, `velocity_bars_at` and `velocity_bars_between`.

### The track panel

A track is not a synth. It owns clips and one child named `instrument`, and any tool with the ports of the note contract fits that slot. So the panel is about the track, and what it shows of the instrument comes from the extension of that instrument.

- The panel is a rack: device cards from left to right, in the order the sound goes through them. The instrument of the track first, then its effects. `device_slots(project, track)` gives the ids of those slots, from the track record.
- For each slot the panel asks the view registry for the card of whatever instance is there: `Views::card_of`, with a `CardFrame` that holds the picker of the slot as the title and, for an effect, the close icon. It names no instrument type, and this crate still depends on no instrument. The view draws the whole device card, because it owns what the body shows. When the slot is empty, or its tool registered no card, the panel draws a card of 200 pt with the same frame and one quiet line: `This track is silent.`, `This slot has no record.` or `This tool has no view.`
- Every card begins with its picker, which is also its title: a quiet dropdown menu whose label is what is in the slot and whose items are what else could go there. Both come from the device registry of the UI SDK (`Devices::label_of`, `Devices::offered`), which whoever makes the window fills, so this crate still knows no instrument and no plugin. The offers are read once per card and not per frame, because a source of them may have to look at the machine. Picking one replaces the whole record of the slot in one commit, named after what was picked (`Choose Six Sines`), so undo brings back what was there. The menu marks what is already in the slot, and picking that does nothing: a fresh record would throw its sound away.
- The card follows the slot live. When an event names a slot and its tool is not the one the card was made for (another `instrument.json` from outside, a delete, an undo), the panel makes the card again. While the tool stays the same, the device view follows its own record.
- The rack follows the track record. A change of the track record, or a child of the track coming or going, makes the panel read the slots again; the rebuild keeps the card of every slot that stays, by its slot id, so a knob drag in one card goes on while another is added, removed or moved next to it. So a reorder written from outside shows in the rack, with no card made again.
- Every menu of the panel, the picker of each card and the control that adds an effect, is
  filled when it is made and filled again when `Devices::offers_generation` changes, through
  one path (`refill_menus`). A source of offers may learn more while the panel is open: the
  plugin host looks for the plugins of this Mac on a thread of its own, so a panel opened at
  the start of a session holds a part of the list and the quiet line that says so.
- At the end of the rack, on the line of the card titles, is a control that adds an effect: the same picker pattern, with what declares itself an effect in it. Picking one is one undo step named after it (`Add Warmth`), and the close icon of an effect card takes it off the track, also one step (`Remove Warmth`).
- The header of an effect card drags it (`CardFrame::draggable`): dropped on another card it takes that card's place, on the instrument it goes first, on `Add effect` last. Cmd-left and cmd-right move the effect whose card has the focus. Each is one undo step, `Move Warmth`, through `move_effect`; a move to where it is already is none. Escape during the drag lets go of the card, and a drop outside the rack moves nothing. The instrument has no grip and stays first. A card is wrapped in a frame whose id names its slot, so a card that moves keeps what it keeps, such as the focus of a knob.
- The panel is 216 pt tall (`track_panel::PANEL_HEIGHT`): 12 above the cards, a card of 192, 12 below. The note editor keeps 352 pt, so a swap between the two moves the lower edge of the timeline. The rack starts 16 pt right of the header column, the cards are 12 pt apart, it scrolls sideways with two fingers, and a 48 pt fade to the window colour at its right edge says that cards go past it.
- The mixer strip of the track is in the header column, on the rows of the cards: the volume, a fader on the meter, at the left from the top of the first row to the value line of the second, the pan knob right of it, and mute and solo on the knob line of the second row. The panel edits it itself, because those values are in the track record. A drag is one gesture and one undo step ("Change volume", "Change pan"), and mute and solo are one commit each ("Mute track", "Unmute track", "Solo track", "Unsolo track"). The bottom of the volume is `-inf`, which the record saves as `"-inf"`. The meter shows what the track sends to the master (`track_peaks`), read once per poll. The panel ends an open drag when it shows another track, when its track is deleted and when it is released, as the note editor does.
- The power icon of an effect card bypasses its slot: one flag in the track record, one undo step, "Turn off <name>" and "Turn on <name>". The card reads whether the slot is on when it draws.
- Apart from that strip the panel edits nothing and keeps no state of the project. The other edits are those of the device views, through the session. A knob drag that is open when the panel closes is finished by the device view when it is released.
- The header column: accent dot and track name on the line of the card titles, the close control at its right, the mixer strip under them. The panel has a focus handle that is no tab stop. It only tells `ArrangementView` whether the focus is inside when the panel closes, so that the focus goes back to the timeline. Tab reaches the close control, the volume, the pan and mute, then the picker and the header icons of the first card and the controls of its device, column by column, and so on.

Not in the rack: sends, buses, and a wet and dry amount.

### The master row and panel

The master row is 40 pt, pinned under the tracks of the timeline (`MASTER_ROW_HEIGHT`), with a ring where a track has its dot. It is drawn by `ArrangementView` and not by the timeline, so it scrolls with nothing. A click, or enter when tab has reached it, opens `MasterPanel` in the place of the track panel or the note editor: the master volume on its meter in the header column, and the Limiter card. The card has expand, which shows the lookahead, and power, which bypasses the limiter, and no close: the limiter is part of the master. Its display shows the last four seconds in 50 columns of 80 ms, the loudest output in green under the ceiling line and the largest reduction hanging from the top, and the handle at the right end of the ceiling line drags the ceiling. Every control is one undo step on the record of the arrangement: "Change master volume", "Change limiter gain", "Change ceiling", "Change release", "Change lookahead", "Turn off Limiter", "Turn on Limiter".

### Editing rules

Every change goes through the session. A drag is one gesture: `begin_gesture` with the first mouse move that changes something, `gesture` per move, `finish_gesture` on mouse up, `cancel_gesture` on escape. So a plain click is no undo step, sound and every view follow each move, the file is written once, and undo and redo wait until the drag ends. A key is one `commit`. The undo labels are "Add clip", "Move clip", "Resize clip", "Delete clip", "Nudge clip", "Draw note", "Move note", "Resize note", "Delete note", "Nudge note", "Cut note", "Paste note", "Duplicate note" and "Change velocity", with an `s` for several, and "Cut clip", "Paste clip", "Duplicate clip", "Draw velocities", "Rename track", "Add tempo change", "Remove tempo change" and "Move <effect>".

- The snap is a setting in the corner above the track headers: off, bar, beat, 1/8, 1/16 or 1/32, a sixteenth when the window opens. It is interface state, like zoom and selection, and not saved: it is a way of working and not part of the piece, so it writes no file and makes no undo step. The timeline and the note editor share it. A drag moves by whole snap steps from where it began and does not snap the result. So a clip or a note that an agent wrote off the grid keeps its offset. Cmd held during a drag bypasses the snap: the drag moves by the pointer's own distance. A new clip and a drawn note start in the grid cell under the pointer. An arrow key moves by one step, and by a sixteenth when snap is off, because a tick is too small to see. A bar and a beat follow the time signature.
- A double click on empty track space adds an empty clip of one bar, named `clip`, `clip-2` and so on.
- A drag of a clip body moves it in time, not before tick 0, and to the track under the pointer. Another track means another id: `move_clip`, a delete and a create in one group. The selection and the open editor go with the clip. A drag that comes back to its first track takes the first id again, so there and back leaves the file where it was. Nothing is written during a drag, so the old file is still there then, and `free_id` would give `part-2`.
- The right edge changes the length. `Clip::set_length` drops the notes that start outside, as the clip rule says. Every move applies to the clip as it was at mouse down, so going in and out again in one drag loses nothing. Undo brings dropped notes back.
- The left edge changes the start and keeps the notes where they are in the project, so their starts change the other way. It drops no note: the edge stops at the first note. It also stops at tick 0 and one snap step before the right edge. This is the simplest correct rule. Cutting a clip inside its notes needs a rule for the cut notes, which comes with splitting clips.
- A clip or a note does not get shorter than one snap step (a sixteenth when snap is off), or than it already was.
- Clips may overlap. There are no collision rules.
- Notes stay inside their clip. A moved note stops where its end meets the clip end, a resized or drawn note ends with the clip at the latest, and a press outside the clip draws nothing. A note that an agent wrote past the clip end stays as it is until it is touched.
- A drawn note has velocity 100. The velocity lane changes it, see "Several notes, copy and paste, velocity".
- When a note edit ends, the notes of the clip are put in order by start and pitch, because the agent doc asks that of whoever writes a clip. The selection stays on its note.
- A drag never leaves the gesture of the session open. The view that closes the editor ends its note drag first, `set_clip` does the same, and both views finish an open gesture when they are released, as a net under every other way to go.
- A clip resize keeps the clip of mouse down and what it wrote last. When the live clip is not what it wrote, something else changed it: an undo under a press that did not move yet, or an agent. The resize then goes on from the live clip, and the grab moves by what the drag had done to its edge. So it never writes an old copy with old notes over a newer clip, and a press without a move changes nothing and keeps redo. A move writes only the start, so it keeps what else changed. Before its first move it takes the live start too.
- A move to another track is a delete and a create in one group of events, and so are its undo and redo. The selection goes to the clip of the same name that the same group created, and the editor goes with it. The editor closes only after the group, when its clip is really gone.
- Outside changes apply during a drag and the next mouse move is the later write. When the dragged clip is deleted from outside, the drag ends and the gesture finishes: the delete was the last write, and undo gives the clip back as it was before the drag. A note drag finds its note again by what it wrote last, and ends when the note is gone. Not handled: a file edit of a clip while that clip is dragged across tracks. Its file still has the old id then, so the edit comes back as a second clip.

### Several clips, copy and paste

- A click selects one clip. Shift-click and cmd-click add a clip or take it out, as in the Finder; a cmd press that moves is a drag without the snap instead, of the selection and the pressed clip, and leaves the selection as it is. A press on empty track space drags a rectangle that selects every clip it touches, and with shift or cmd it adds them. Escape during it puts back what was selected before. Cmd-a selects every clip. A plain click on one of several selected clips, without a move, selects it alone. Selection is interface state: nothing is written.
- A drag of a selected clip moves every selected clip by the same distance in time and in track rows. The earliest stops at tick 0 and the outer ones at the first and the last track, and the others keep their distance. It is one gesture and one undo step, "Move clips". Each clip follows the rules of a single one: another track is a delete and a create, and back on its first track it takes its first id again. On another track a clip takes its name without a number at its end where that is free, so `clip` moved down onto a track that has a `clip` is `clip-2` there, and `clip` again when an arrow brings it back. A selected clip deleted from outside during the drag leaves it; the drag ends only when that is the clip under the pointer. A resize is of the clip under the pointer only.
- Delete and backspace delete every selected clip, the arrows move every one, and each is one undo step: "Delete clips", "Nudge clips". An arrow that would take one of them past the first or the last track moves none.
- Cmd-c copies the selected clips into the clipboard of the window. It is in the app only, never on the system clipboard, and it goes with the session. Cmd-x copies and deletes, "Cut clips". Cmd-v pastes at the playhead, the top row of what was copied onto the track of the first selected clip, else onto the selected track, else onto the first track, "Paste clips". The clips keep their distances in time and in rows. A row that would fall below the last track lands on the last track, so nothing that was copied is lost; clips may overlap. Cmd-d puts a copy right after the selected clips on the same tracks and keeps the clipboard, "Duplicate clips". The copies are selected. Each of these is one undo step, and one clip has the name without the `s`.
- Tracks are not copied. A track owns an instrument and effects of any tool, and a plugin with a state file, and the core creates a record only of a type the caller knows. A track is added with "Add track" and its devices picked in its panel.
- A clip written, moved or deleted from outside is part of the selection, or leaves it, at once.

### Several notes, copy and paste, velocity

The note editor follows the rules of clips in "Several clips, copy and paste", with notes for clips.

- A click selects one note, shift-click and cmd-click add one or take it out, a press on empty space drags a rectangle that selects every note it touches (with shift or cmd it adds them), and cmd-a selects every note of the clip. A double click on empty space inside the clip adds a note of one unit of the grid; its second press drags its length.
- A drag of a selected note moves every selected note, and the arrows move every one, as a whole: the first to meet the clip start, the clip end, pitch 0 or 127 stops them all. A resize is of the note under the pointer. Delete deletes every selected note. "Move notes", "Nudge notes", "Delete notes".
- Cmd-c copies the selected notes into the clipboard of the window, the one the timeline uses, and cmd-x cuts. Cmd-v pastes at the playhead when it is inside the clip, else right after the selected notes, else at the start of the clip; a note that would start past the end of the clip is left out. Cmd-d puts a copy right after the selected notes. The pasted notes are selected. Each is one undo step.
- The velocity lane: a press on a bar drags the velocity of its note, and of every selected note when its note is selected, by the same distance. Of a chord it takes the bar whose top is nearest the pointer. A press off the bars draws across the lane. Alt-up and alt-down step the selected velocities by 10. A velocity is 1 to 127.
- An undo or a redo selects the notes it brought back, and an undo of a delete or cut of clips selects those clips again.

### Renaming a track

A double click on a track header, or enter while a track and no clip is selected, opens a name field in the header with the name selected. Enter gives the name and a click anywhere else too, one undo step "Rename track"; escape leaves it as it was. An empty name is no name and the track keeps the one it had. The folder of the track keeps its name: the id of a track never changes. While the field is open the keys are the field's: space types a space and does not play. A track deleted from outside while its field is open closes the field.

### Tempo changes in the ruler

Every tempo change after tick 0 has a mark in the ruler: a line at its tick and a label, `96 bpm`, which starts after the bar number when it is on a bar line and covers the bar numbers under it otherwise. The change at tick 0 is the tempo of the transport and has no mark.

- A double click in the ruler adds a tempo change on the grid there, and `t` adds one at the playhead, on the nearest step of the grid while the project plays. It plays the tempo that played there already, so nothing sounds different until it is edited. One undo step, "Add tempo change". Where there is one already, it is selected.
- A click on a mark selects the tempo change and moves the playhead onto it, so the tempo of the transport is its tempo: a drag of that number or its arrows edit it, the shared drag number of the transport and one undo step "Change tempo", as before. Delete or backspace removes the selected one, "Remove tempo change". A click on the ruler elsewhere moves the playhead to the grid and selects no tempo change, and so does escape.
- A tempo change written or removed from outside shows or goes at once, and a removed one is no longer selected.
- Not built: dragging a mark in time, tempo ramps and time signature changes.

### Keys and mouse of the timeline

| Keys or mouse | What they do |
| --- | --- |
| click, shift-click, cmd-click on a clip | Select it, add it or take it out |
| drag on empty track space | Select the clips the rectangle touches, with shift or cmd add them |
| cmd-a | Select every clip |
| drag a selected clip | Move every selected clip. Cmd held bypasses the snap |
| delete, backspace | Delete the selected clips, or the selected tempo change |
| left, right | Move the selected clips by a unit of the grid |
| up, down | Move the selected clips a track, or select the track above or below |
| cmd-c, cmd-x, cmd-v, cmd-d | Copy, cut, paste at the playhead, duplicate after the selection |
| enter | Open the note editor of the first selected clip, or edit the name of the selected track |
| cmd-down | Open the note editor of the first selected clip, or the panel of the selected track |
| double click on a track header | Edit its name |
| double click in the ruler, or `t` | Add a tempo change there, or at the playhead |
| click a tempo mark | Select it and move the playhead onto it |
| escape | Cancel a drag or a rectangle, else let go of a tempo change, else close the panel below |

### Keys and mouse of the note editor

| Keys or mouse | What they do |
| --- | --- |
| click, shift-click, cmd-click on a note | Select it, add it or take it out |
| drag on empty space | Select the notes the rectangle touches, with shift or cmd add them |
| double click on empty space in the clip | Add a note; the second press drags its length |
| cmd-a | Select every note |
| drag a selected note, or its end | Move every selected note, or resize this one. Cmd held bypasses the snap |
| delete, backspace | Delete the selected notes |
| left, right, up, down, shift-up, shift-down | Move the selected notes by a unit of the grid, a semitone, an octave |
| cmd-c, cmd-x, cmd-v, cmd-d | Copy, cut, paste, duplicate after the selection |
| drag a velocity bar, or across the lane | Change the velocity of its note and the selected ones, or draw |
| alt-up, alt-down | The selected velocities by 10 |
| click the ruler | Move the playhead there, where a paste goes |
| escape | Cancel a drag or a rectangle, else close the editor |

## Summary

The arrangement registers a summary (`ToolRegistration::summary`), which `runtime <folder> --inspect` prints: each track in order with name, colour, order and instrument tool, and each clip with its id, `bar:beat:tick` range, tick range, note count and pitch range.

## Checks

```sh
cargo nextest run -p arrangement -p runtime
cargo nextest run -p arrangement --test arrangement mixer                         # gain, pan and mute with numbers
cargo nextest run -p arrangement --test arrangement effects                       # the chain and its order, with numbers
RTSAN_ENABLE=1 cargo nextest run -p arrangement                                   # with the realtime sanitizer
cargo nextest run -p runtime --test window                                       # the views with a simulated mouse and keys
cargo nextest run -p runtime --test window track_panel                           # the track panel and the synth view in it
cargo nextest run -p runtime --test window editing                               # several clips, copy and paste, rename, tempo changes, snap
cargo nextest run -p runtime --test window several_notes velocity                # several notes, copy and paste, the velocity lane
cargo nextest run -p runtime --test window rack                                  # reordering the rack
cargo nextest run -p runtime --test projects rack_order                          # a reorder renders as the written order
cargo nextest run -p runtime --run-ignored only hundred_tracks --no-capture      # 100 tracks of 100 clips
cargo test -p runtime --test snapshots                                           # the window as PNGs, with frame times
```

Measured September 19, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline: a project of 100 tracks with 100 clips each (10,200 records, an eighth of the tracks playing a chord at any time) opens in 0.44 s, applies one outside clip edit in 0.5 ms, and plays 62 times faster than realtime.

Measured again September 20, 2026, with the stereo path and the mixer: the same project opens in 0.44 s, applies an edit in 0.5 ms and plays 34 times faster than realtime. Nearly all of that is the stereo buffers, which are twice as many samples per port: without the mixer in the path it is 35 times. Most synths in that project are idle and return at once, so what is left is the engine moving buffers around. Reusing buffers in `compile`, which ENGINEERING.md section 3 leaves open, is where to look if this ever matters.
