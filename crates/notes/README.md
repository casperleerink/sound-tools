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

`pedal` is a list of `PedalChange`, each a `start` in ticks from the clip start and a `value`. It follows the same rules as the notes, and `clip.placed_pedal()` gives them at their project position. A clip with no pedal leaves the field out of its JSON, so a clip written before the pedal existed loads and is written back byte for byte as it was. `Clip::new(start, length, notes)` makes a clip without pedal, which is every clip that was not recorded.

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

## The ports of an instrument

An instrument is any tool whose behaviour names these two ports. An owner finds them by name, `context.child_input("instrument", NOTES_INPUT)`, so any tool with these ports fits.

| Constant | Port name | Kind |
| --- | --- | --- |
| `NOTES_INPUT` | `notes` | event input carrying `NoteEvent` |
| `AUDIO_OUTPUT` | `audio` | audio output, stereo like every audio port |

`extensions/instrument/tests/synth/support.rs` has a complete small sender: a test track tool with a sequencer processor.
