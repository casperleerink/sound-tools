# Instrument: the synth

`instrument.synth` is a subtractive synth with 16 voices: an oscillator, a low-pass filter and an envelope per voice. As the `instrument.json` of a track it plays the notes of that track.

```json state/arrangement/piano/instrument.json
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
    "gain": 0.15
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default synth.

| Field | Meaning | Values |
| --- | --- | --- |
| `waveform` | The oscillator. `"saw"` is bright and full, `"square"` is hollow. | `"saw"`, `"square"` |
| `cutoff_hz` | The filter lets through what is below this frequency. Lower is darker. | 20 to 20000 |
| `resonance` | A peak at the cutoff. 0 is none, 1 is a strong ringing peak. | 0 to 1 |
| `attack_seconds` | From note on to full level. | 0.001 to 10 |
| `decay_seconds` | From full level down to the sustain level. | 0.001 to 10 |
| `sustain` | The level a held note settles at. 0 makes every note a pluck. | 0 to 1 |
| `release_seconds` | From note off to silence. | 0.001 to 10 |
| `gain` | Linear output gain. Tracks add up and there is no mixer yet, so keep it near 0.15, and lower for thick chords. | 0 to 1 |

Starting points: a bass is `cutoff_hz` 300 to 800 with `resonance` near 0.4. A pluck is `sustain` 0 with `decay_seconds` 0.15 to 0.4. A pad is `attack_seconds` 0.5 or more and `release_seconds` 1 or more.

An edit applies while notes sound, without a click. A synth outside a track is `state/<name>.json`, has the ports `notes` (in) and `audio` (out, mono), and needs a connection in `project.json` to be heard.
