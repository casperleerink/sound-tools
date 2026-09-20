# instrument

The bundled instrument extension. It has one tool, `instrument.synth`: a subtractive synth that plays the note events of the `sound-notes` contract (`crates/notes/README.md`).

Enable it in `project.json` under `extensions` as `"instrument"`.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and some starting points for sounds. The runtime writes that file into every project as part of `AGENTS.md`, and a test loads its example, so it is the single source for the format.

The synth owns no children, so an instance is always one file: `state/<name>.json`, or `instrument.json` inside the folder of the track that owns it. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field or a value out of range does not load. The instance then keeps its last valid state, and the problem names the file and the field, for example `state: cutoff_hz must be from 20 to 20000, not 5`.

With the defaults one note peaks at 0.16 for velocity 127 and at 0.10 for velocity 100. A chord adds up: six notes at velocity 127 peak at 0.47 with the saw and 0.67 with the square.

An edit applies while notes are held. The notes go on. Gain, cutoff and resonance move to the new value over 0.05 s, so an edit does not click. A sustain edit glides at the decay speed. Attack, decay and release times apply at once, also to held notes. A waveform edit is a switch, not a fade.

## Ports

| Name | Kind |
| --- | --- |
| `notes` | event input carrying `sound_notes::NoteEvent` |
| `audio` | mono audio output |

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
RTSAN_ENABLE=1 cargo nextest run -p instrument                                      # with the realtime sanitizer
cargo nextest run -p instrument --run-ignored only realtime_ratio --no-capture     # speed of 100 synths
cargo nextest run -p instrument --run-ignored only real_device --no-capture        # plays on the default device
```

Measured September 19, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, one engine, offline: 100 synths that each hold a chord of four notes render 13 times faster than realtime with the saw and 11 times with the square. 100 idle synths, with their 100 idle senders, render 250 times faster than realtime. The frame loop of a voice is bound by the chain of operations in the filter from one frame to the next. The next step, if it is ever needed, is to run four voices in one SIMD loop.
