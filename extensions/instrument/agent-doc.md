# Instrument: the synth

`instrument.synth` is a subtractive synth with 16 voices: an oscillator, a low-pass filter and an envelope per voice. It plays the notes that reach its input `notes`.

Here soft keys on the track `piano`:

```json state/song/piano/synth.json
{
  "tool": "instrument.synth",
  "state": {"cutoff_hz": 1200.0, "attack_seconds": 0.01, "release_seconds": 0.8}
}
```

{{fields instrument.synth}}

Starting points: a bass is `cutoff_hz` 300 to 800 with `resonance` near 0.4, `"warm"` for a rounder one. A pluck is `sustain` 0 with `decay_seconds` 0.15 to 0.4. A pad is `attack_seconds` 0.5 or more and `release_seconds` 1 or more.

Every number here can move over time with a `lane` child of its parent (`agent-docs/lane.md`).

An edit applies while notes sound, without a click. It has the ports `notes` (in) and `audio` (out, stereo): a parent connects them, or, at the top, `state/<name>.json`, a connection in `project.json`.
