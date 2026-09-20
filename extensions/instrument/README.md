# instrument

The bundled instrument extension. It has one tool, `instrument.synth`: a subtractive synth that plays the note events of the `sound-notes` contract (`crates/notes/README.md`).

Enable it in `project.json` under `extensions` as `"instrument"`.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and some starting points for sounds. The runtime writes it into every project as `agent-docs/instrument.md`, listed in the map `AGENTS.md`, and a test loads its example, so it is the single source for the format.

The synth owns no children, so an instance is always one file: `state/<name>.json`, or `instrument.json` inside the folder of the track that owns it. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field or a value out of range does not load. The instance then keeps its last valid state, and the problem names the file and the field, for example `state: cutoff_hz must be from 20 to 20000, not 5`.

With the defaults one note peaks at 0.16 for velocity 127 and at 0.10 for velocity 100. A chord adds up: six notes at velocity 127 peak at 0.47 with the saw and 0.67 with the square.

An edit applies while notes are held. The notes go on. Gain, cutoff and resonance move to the new value over 0.05 s, so an edit does not click. A sustain edit glides at the decay speed. Attack, decay and release times apply at once, also to held notes. A waveform edit is a switch, not a fade.

## The view

`view::register(views)` registers `SynthView` for `instrument.synth`, as the arrangement registers its view. `view.rs` is the only module here that uses GPUI. Whatever hosts the view gives it a surface. The track panel of the arrangement puts it into a device card, and it does not know this crate: it asks the view registry for the view of the instance in the `instrument` slot of a track.

The view shows the title `Synth` and one control per field, in four groups parted by air: oscillator, filter, envelope, output. The waveform is a segmented control. Every number is a knob with its label and its value under it.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `cutoff_hz` | Cutoff | 20 to 20000 | 2000 | logarithmic | `632 Hz`, `2 kHz` |
| `resonance` | Resonance | 0 to 1 | 0.2 | linear | `20%` |
| `attack_seconds` | Attack | 0.001 to 10 | 0.005 | logarithmic | `5 ms`, `1.5 s` |
| `decay_seconds` | Decay | 0.001 to 10 | 0.2 | logarithmic | `200 ms` |
| `sustain` | Sustain | 0 to 1 | 0.7 | linear | `70%` |
| `release_seconds` | Release | 0.001 to 10 | 0.3 | logarithmic | `300 ms` |
| `gain` | Gain | 0 to 1 | 0.15 | linear | `15%` |

The range and the default of a field are written once, in the `Parameter` constants next to `SynthState` (`CUTOFF`, `RESONANCE`, `ATTACK`, `DECAY`, `SUSTAIN`, `RELEASE`, `GAIN`, and all of them in `PARAMETERS`). `validate`, `Default`, the range of each knob and what a double click resets to all read them. A test holds this table and the one in `agent-doc.md` to them. This is local to the crate on purpose. It is not the declarative parameter system of ARCHITECTURE.md, which is still open. What is only about the interface is in `view.rs`: the label, the unit, the travel and the name of the undo step.

Frequencies and times are heard in ratios, so their knobs travel in ratios: a third of the cutoff knob is a decade. A knob gives values of three significant digits, so the file stays short: `"cutoff_hz": 632.0`. A value that an agent wrote with more digits is kept until the knob moves up or down: a press, a press with a sideways move, and a drag there and back all leave it as it is, to the digit.

Editing:

- A knob drag is one gesture of the session. It begins with the first mouse move that changes the value, publishes per move, so the sound follows during the drag, and ends as one undo step: "Change cutoff", "Change resonance", "Change attack", "Change decay", "Change sustain", "Change release", "Change gain". The file is written once, at the end. Escape cancels. A press without a move is no step, and a drag there and back is none either.
- A double click sets the default. An arrow key is one step of a fiftieth of the travel, with shift a five-hundredth. A waveform switch is "Change waveform". Each is one commit.
- The view keeps no copy of the state. Its one field of its own says whether a drag has the gesture open. An outside edit of the record shows at once, also during a drag, and the next mouse move is the later write. It works from the value at the press, and it writes only its own field, so what else the file changed is kept.
- When the record is deleted under a drag, the gesture finishes and does not cancel: the delete was the last write, and undo gives the synth back as it was before the drag. When the view is released during a drag, because the panel closed, it finishes the gesture too.
- The callbacks of the controls hold the view weakly. `cx.processor` holds it strongly, and the mouse listeners of the last frame would then keep a closed view, and its open drag, alive for one more frame.

## Ports

| Name | Kind |
| --- | --- |
| `notes` | event input carrying `sound_notes::NoteEvent` |
| `audio` | audio output, stereo. The synth is one bank of voices in the middle: the same samples in both channels. Where a track puts it is the track's business |

A track connects both itself. To play a synth straight to the device, add connections to `project.json`:

```json
{"from": {"instance": "lead", "port": "audio"}, "to": {"device_output": 0}}
```

## How it plays

- 16 voices. Each is one oscillator, one low-pass filter of 12 dB per octave and one envelope. The 17th note takes over a voice: the quietest one that was already released, by its level at that moment, or the oldest held one. The voice keeps its phase and its loudness, so the takeover does not click.
- Note on and note off apply on their exact frame. Velocity sets the level with a square curve: velocity 64 is a quarter of velocity 127.
- The loudest single note is one on the cutoff with `resonance` 1. At `gain` 0.25 and velocity 127 it peaks at 0.38 with the saw and 0.75 with the square.
- A held note with `sustain` 0 ends by itself after its decay. Its voice is then free, and the later note off does nothing.
- `Off` releases every held note of its pitch. `AllOff` releases everything. Release tails always sound to their end.
- A synth with no sounding voice does no work. A voice ends when its release reaches silence, and the output is then exactly zero.
- Everything is allocated when the synth is created. `process` never allocates.

## Checks

```sh
cargo nextest run -p instrument -p sound-notes
cargo nextest run -p runtime --test window track_panel                              # the view, with a simulated mouse and keys
RTSAN_ENABLE=1 cargo nextest run -p instrument                                      # with the realtime sanitizer
cargo nextest run -p instrument --run-ignored only realtime_ratio --no-capture     # speed of 100 synths
cargo nextest run -p instrument --run-ignored only real_device --no-capture        # plays on the default device
```

Measured September 19, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, one engine, offline: 100 synths that each hold a chord of four notes render 13 times faster than realtime with the saw and 11 times with the square. 100 idle synths, with their 100 idle senders, render 250 times faster than realtime. The frame loop of a voice is bound by the chain of operations in the filter from one frame to the next. The next step, if it is ever needed, is to run four voices in one SIMD loop.
