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

In Rust, `Pitch`, `Velocity` and `Length` cannot hold a wrong value: `Pitch::new(60)?`, `Velocity::new(100)?`, `Length::new(Ticks(480))?`. `Pitch::frequency_hz()` gives the frequency, twelve equal steps per octave with A4 at 440 Hz. `start` is the core `Ticks`. A length of 0 cannot be built, because such a note would get its off before its on and never end. `note.end()` is the tick of the note off. `note.on()` and `note.off()` give the events below.

The record that holds notes, such as a clip, belongs to the extension that saves it.

## The note event

`NoteEvent` is what travels between processors on the audio thread. It is `Copy`, so it is a core event: declare `EventOutput::<NoteEvent>` on the sender and `EventInput::<NoteEvent>` on the instrument.

- `On { pitch, velocity }` starts a note.
- `Off { pitch }` releases every held note of that pitch.
- `AllOff` releases every held note. Release tails still sound.

Rules for a sender:

- Send each event at the frame of its tick: `context.transport.offset_of(tick)`, see "Schedule from the transport" in `crates/core/README.md`.
- A sender keeps no list of held notes. When the transport says `stopped_playing` or `jumped`, send one `AllOff` at offset 0, before the notes of that block.
- On one frame, send the offs before the ons. Else the end of one note releases the next note of the same pitch that starts there.

Known limit: `AllOff` releases everything the instrument holds, whoever started it. Once MIDI input feeds the same `notes` port, a transport stop would also release the notes held on the keyboard. This is to be decided with MIDI input.

Rules for an instrument:

- An `On` for a pitch that is already held starts another note of that pitch. One `Off` releases both.
- More notes than the instrument has voices is not an error. The instrument decides which note gives way.

## The ports of an instrument

An instrument is any tool whose behaviour names these two ports. An owner finds them by name, `context.child_input("instrument", NOTES_INPUT)`, so any tool with these ports fits.

| Constant | Port name | Kind |
| --- | --- | --- |
| `NOTES_INPUT` | `notes` | event input carrying `NoteEvent` |
| `AUDIO_OUTPUT` | `audio` | mono audio output |

`extensions/instrument/tests/synth/support.rs` has a complete small sender: a test track tool with a sequencer processor.
