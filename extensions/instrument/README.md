# instrument

The bundled instrument extension. It has one tool, `instrument.synth`: a subtractive synth that plays the note events of the `sound-notes` contract (`crates/notes/README.md`).

Enable it in `project.json` under `extensions` as `"instrument"`.

## The record

The synth owns no children, so an instance is always one file: `state/<name>.json`, or `instrument.json` inside the folder of the track that owns it. A field you leave out takes its default, so `"state": {}` is the default synth. The runtime writes every field when it saves. An unknown field or a value out of range does not load. The instance then keeps its last valid state, and the problem names the file and the field, for example `state: cutoff_hz must be from 20 to 20000, not 5`.

```json
{
  "tool": "instrument.synth",
  "state": {
    "waveform": "saw",
    "cutoff_hz": 2000.0,
    "resonance": 0.2,
    "attack_seconds": 0.005,
    "decay_seconds": 0.2,
    "sustain": 0.7,
    "release_seconds": 0.3,
    "gain": 0.25
  }
}
```

These are also the defaults.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `waveform` | The oscillator. `"saw"` is bright and full. `"square"` is hollow. | `"saw"`, `"square"` | `"saw"` |
| `cutoff_hz` | The low-pass filter lets through what is below this frequency. Lower is darker. | 20 to 20000 Hz | 2000 |
| `resonance` | A peak at the cutoff. 0 is none, 1 is a strong ringing peak. It adds level. | 0 to 1 | 0.2 |
| `attack_seconds` | From note on to full level. | 0.001 to 10 s | 0.005 |
| `decay_seconds` | From full level down to the sustain level. | 0.001 to 10 s | 0.2 |
| `sustain` | The level a held note settles at, as a part of full level. 0 makes every note a pluck. | 0 to 1 | 0.7 |
| `release_seconds` | From note off to silence. | 0.001 to 10 s | 0.3 |
| `gain` | Linear output gain. With the defaults one note peaks at 0.35 for velocity 127 and at 0.22 for velocity 100. A chord adds up. | 0 to 1 | 0.25 |

Some starting points: a pluck is `sustain` 0 with `decay_seconds` 0.15 to 0.4. A pad is `attack_seconds` 0.5 or more and `release_seconds` 1 or more. A bass is `cutoff_hz` 300 to 800 with `resonance` near 0.4.

An edit applies while notes are held. The notes go on. Gain, cutoff and resonance move to the new value over 0.05 s, so an edit does not click. A sustain edit glides at the decay speed. Attack, decay and release times apply from the next frame. A waveform edit is a switch, not a fade.

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

- 16 voices. Each is one oscillator, one low-pass filter of 12 dB per octave and one envelope. The 17th note takes over a voice: the quietest one that was already released, or the oldest held one. The voice keeps its phase and its loudness, so the takeover does not click.
- Note on and note off apply on their exact frame. Velocity sets the level with a square curve: velocity 64 is a quarter of velocity 127.
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
