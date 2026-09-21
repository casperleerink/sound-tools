# sound-notes

The note contract. A tool that sends notes, such as a track, and a tool that plays them, such as a synth, both depend on this crate and not on each other. The core knows nothing about notes.

## The saved note

`Note` is one note in a saved record, for example one line of a clip:

```json
{"start": 0, "length": 480, "pitch": 60, "velocity": 100}
```

| Field | Meaning | Values |
| --- | --- | --- |
| `start` | Where the note starts, in ticks from the start of what holds it. 960 ticks are a quarter note. | whole number, 0 or more |
| `length` | How long the note is held, in ticks. | whole number, 1 or more |
| `pitch` | MIDI note number. 60 is middle C, 69 is A4 at 440 Hz. One step is a semitone. | 0 to 127 |
| `velocity` | How hard the note is played. | 1 to 127 |

This is exactly how the runtime writes a note: spaces after `:` and `,`, and one note per line in a list of notes. Write it the same way, so an edit by an agent and a save by the runtime make no whitespace diff. The compact form without spaces loads too, like any valid JSON.

All four are plain JSON numbers. A value out of range, a fraction or an unknown field does not load, and the message names the range: `pitch must be from 0 to 127, not 128`.

In Rust, `Pitch`, `Velocity` and `Length` cannot hold a wrong value: `Pitch::new(60)?`, `Velocity::new(100)?`, `Length::new(Ticks(480))?`. `Pitch::frequency_hz()` gives the frequency, twelve equal steps per octave with A4 at 440 Hz. `start` is the core `Ticks`. A length of 0 cannot be built, because such a note would get its off before its on and never end. `note.end()` is the tick of the note off. `note.on()` and `note.off()` give the events below. Interface math that cannot fail uses `Pitch::nearest(number)`, `Velocity::nearest(number)` and `Length::at_least_one(ticks)`: the valid value nearest to any number, for a drag that stops at the ends.

## The sustain pedal

`Pedal` is how far the pedal was pressed, 0 to 127, as it was played. It is a number and not a bool so that a piano plugin that knows half pedal loses nothing. `Pedal::UP` is 0, and `pedal.is_down()` is true from `Pedal::DOWN_FROM` (64), which is MIDI's rule for controller 64.

An instrument with one damper, such as the synth here, only asks `is_down()`: while the pedal is down a note off does not release, and the note sounds until the pedal comes up.

## The saved clip

`Clip` is the record `arrangement.clip`: `start` and `length` in ticks, `notes`, and `pedal` when it was recorded. It lives here because its saved form is what other extensions read. `extensions/arrangement/agent-doc.md` has the format with a complete example. The runtime writes it into a project as `agent-docs/arrangement.md`.

The rules are the same for everyone who plays or draws a clip. Note starts count from the clip start. Every note starts inside the clip, below its `length`, else the record does not load. A note that is longer than the rest of the clip ends where the clip ends. `clip.placed_notes()` gives the notes at their project position with these rules applied. `clip.set_length(length)` drops the notes a shorter clip cannot hold. `Length` is the type of both lengths: 1 tick or more. A clip lives directly inside a track (`TRACK_TOOL`, `arrangement.track`). Anywhere else it does not load, because nothing would play it.

`pedal` is a list of `PedalChange`, each a `start` in ticks from the clip start and a `value`. It follows the same rules as the notes: it starts inside the clip, and it ends with the clip, as a note longer than the rest of its clip ends there. `clip.placed_pedal()` gives the moves at their project position. A clip with no pedal leaves the field out of its JSON, so a clip written before the pedal existed loads and is written back byte for byte as it was.

`take` is the raw take a recorded clip came from: the name of a file under `assets/takes/`, without `.json`. It is a saved reference, so it owns nothing and keeps nothing alive, and it travels with the clip through a move, a rename, a resize and a copy. A clip that was not recorded leaves it out. `Clip::is_valid_take_name` is the rule for the name, which `validate` applies: it becomes a file name, so it can never point outside the project folder.

`Clip::new(start, length, notes)` makes a clip with no pedal and no take, which is every clip that was not recorded.

## The note event

`NoteEvent` is what travels between processors on the audio thread. It is `Copy`, so it is a core event: declare `EventOutput::<NoteEvent>` on the sender and `EventInput::<NoteEvent>` on the instrument.

- `On { pitch, velocity }` starts a note.
- `Off { pitch }` releases every held note of that pitch, unless the pedal holds it.
- `Pedal(value)` moves the sustain pedal.
- `AllOff` releases every held note and puts the pedal up. Release tails still sound.

Rules for a sender:

- Send each event at the frame of its tick: `context.transport.offset_of(tick)`, see "Schedule from the transport" in `crates/core/README.md`.
- When the transport says `stopped_playing` or `jumped`, send one `AllOff` at offset 0, before the notes of that block.
- A sender whose notes never change while they sound needs no list of held notes. One whose notes can be edited, moved or deleted while they sound keeps a fixed list of what it started, with the tick of each off, and sends offs from that list. `extensions/arrangement/src/sequencer.rs` does this.
- On one frame, send the offs before the ons. Else the end of one note releases the next note of the same pitch that starts there.

Known limit, decided with MIDI input in step 3 of the second milestone: `AllOff` releases everything the instrument holds, whoever started it. MIDI input feeds the same `notes` port, so a transport stop or a seek also releases the notes held on the keyboard and puts the pedal up. We accept that. It is what makes "no note is ever stuck" a property of the contract and not of every sender, and while recording it is what a composer wants anyway: the take ends there.

- A sender that plays the pedal sends its value when it changes, and before the notes of the same frame, so an off on that frame sees where the pedal stands. It never has to send the pedal up itself on a stop or a seek: the `AllOff` it already sends does that.

Rules for an instrument:

- An `On` for a pitch that is already held starts another note of that pitch. One `Off` releases both.
- More notes than the instrument has voices is not an error. The instrument decides which note gives way.
- While the pedal is down (`Pedal::is_down`), an `Off` does not release: the note sounds until the pedal comes up. An `AllOff` puts the pedal up as well, so a stop, a seek or an edit can leave no note hanging under a pedal nobody will lift.
- An instrument that has no damper may ignore `Pedal`. One that knows half pedal gets the value as it was played.

## The ports of an instrument and of an effect

An instrument is any tool whose behaviour names these two ports. An owner finds them by name, `context.child_input("instrument", NOTES_INPUT)`, so any tool with these ports fits.

| Constant | Port name | Kind |
| --- | --- | --- |
| `NOTES_INPUT` | `notes` | event input carrying `NoteEvent` |
| `AUDIO_OUTPUT` | `audio` | audio output, stereo like every audio port |

An effect is any tool with `AUDIO_INPUT` and `AUDIO_OUTPUT`: the sound of whatever comes before it goes in and what it makes comes out. The input has the same name as the output, `audio`, because inputs and outputs are named apart, so a chain reads as `audio` to `audio` and no tool has to invent a name for the one thing it takes.

These names are here, in the note contract crate, and not in a crate of their own, because this is already the crate both sides of a track read and neither depends on the other: the arrangement finds the effects of a track by these names (since step 6 of the second milestone) and the plugin host declares them, and neither knows the other exists. A crate for two constants would buy nothing.

A tool may have all three ports. The plugin host does, whatever the plugin is, because one record serves an instrument slot and an effect slot and the host knows nothing of slots. An instrument's audio input is connected to nothing and is silent.

`extensions/instrument/tests/synth/support.rs` has a complete small sender: a test track tool with a sequencer processor. `extensions/arrangement/tests/arrangement/support.rs` has a complete small effect, `Trim`.

## The saved raw take

`RawTake` is the performance a recording writes, under `assets/takes/<name>.json`. It lives here and not in the MIDI extension because two extensions read it and neither may depend on the other: MIDI writes one when a recording ends, and `extensions/fit-tempo` reads one to find the beats and to make the clip again.

```json
{"start_us": 0, "end_us": 2000000, "start_tick": 0, "end_tick": 3840, "pedal_at_start": 0,
 "events": [{"kind":"on","time_us":15230,"sounded_us":16000,"pitch":60,"velocity":88}]}
```

Every time is microseconds, never ticks, because a tick means nothing without a tempo map and the whole point of a fit is that the tempo map changes. `start_us` and `end_us` are where the recording began and ended on the project timeline. Each message carries two times, both counted from the start of the recording: `time_us`, when it reached this process, which is the performance as it was played and what a beat finder works from, and `sounded_us`, when the engine really sounded it, which is the start of the audio block that carried it and what a clip has to reproduce to sound the same. `start_tick` and `end_tick` are the same two moments under the tempo map of that moment, for reading only.

`RawTake::clip(tick_of)` is the one place that turns a performance into a clip. `tick_of` is a function from a place on the project timeline in microseconds to a tick, so recording calls it with the clock of the moment and a fit calls it with the clock of the fitted map. The rules are the same either way: a note still held at the end ends there, a note off with no note on before it is left out, a pedal move that changes nothing is left out, a take that began under a held pedal starts with that value, and one that ends with the pedal down lifts it at its end.

`RawTake::write(assets)` writes it under a name of its own through `Assets::create`, which never opens a file that exists, and `RawTake::read(assets, name)` reads one back and checks it with `RawTake::validate`. A take file is a file an agent reads and someone can damage, so times that are not times are refused before any reader lays out an array over them: a recording longer than `MAX_TAKE_MICROS` (an hour), a moment past `MAX_PROJECT_MICROS`, an end before the start, or messages out of the order they arrived. The file itself is never rewritten. `take_asset(name)` is the `AssetName`. A take written before this crate saved `start_us`, `end_us` and `sounded_us` does not load, and the fit says so.
