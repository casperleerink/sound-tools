# Instrument: the synth

`instrument.synth` is a simple subtractive synth. Its ports are `notes` (in) and `audio` (out, stereo).

Here soft keys on the track `piano`:

```json state/song/piano/synth.json
{
  "tool": "instrument.synth",
  "state": {"cutoff_hz": 1200.0, "attack_seconds": 0.01, "release_seconds": 0.8}
}
```

{{fields instrument.synth}}

Starting points: a bass is `cutoff_hz` 300 to 800, `resonance` near 0.4, `"warm"` for a rounder one. A pluck: `sustain` 0, `decay_seconds` 0.15 to 0.4. A pad: `attack_seconds` 0.5 or more, `release_seconds` 1 or more.
