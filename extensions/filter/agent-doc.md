# Filter: the built-in filter effect

`filter` is an effect: it takes the sound of a track and lets part of it through. Low pass makes a sound darker, high pass thinner, band pass leaves a band in the middle, notch takes one out. Resonance adds a peak at the cutoff, drive warms the sound up before the filter, and an LFO can move the cutoff up and down.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the bass plays through a filter named `dark`:

```json state/arrangement/bass/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Bass",
    "colour": "peach",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["dark"]
  }
}
```

```json state/arrangement/bass/dark.json
{
  "tool": "filter",
  "state": {
    "type": "low_pass",
    "cutoff_hz": 1000.0,
    "resonance": 0.2,
    "slope": 12,
    "drive_db": 0.0,
    "mix": 1.0,
    "lfo_rate_hz": 1.0,
    "lfo_depth_octaves": 0.0
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default filter.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `type` | What the filter lets through. | `"low_pass"`, `"band_pass"`, `"high_pass"`, `"notch"` | `"low_pass"` |
| `cutoff_hz` | Where the filter starts to cut, or the middle of the band or the notch. | 20 to 20000 | 1000 |
| `resonance` | A peak at the cutoff, and a narrower band or notch. 0 is none, 1 rings strongly. | 0 to 1 | 0.2 |
| `slope` | How steeply it cuts, in dB per octave. 24 is steeper and more synth-like. | `12` or `24` | `12` |
| `drive_db` | Gain into a soft saturation before the filter. 0 is clean. It makes quiet sounds louder too. | 0 to 24 | 0 |
| `mix` | 1 is only the filtered sound, 0 is the sound as it came in. | 0 to 1 | 1 |
| `lfo_rate_hz` | How fast the LFO moves the cutoff up and down. | 0.05 to 20 | 1 |
| `lfo_depth_octaves` | How far the LFO moves the cutoff each way. 0 is no LFO. | 0 to 4 | 0 |

Starting points: darker is `low_pass` with `cutoff_hz` 400 to 2000. Thinner, for a sound that fights the bass, is `high_pass` at 150 to 400. A telephone voice is `band_pass` at 1500 with `resonance` 0.4. A wah is `band_pass` or `low_pass` with `resonance` 0.6, `lfo_rate_hz` 2 and `lfo_depth_octaves` 1.5. An acid bass is `low_pass`, `slope` 24, `resonance` 0.7 to 0.9 and some `drive_db`.

An edit applies while the track plays and glides over 20 ms, so it does not click. At the cutoff a `low_pass` or `high_pass` is 3 dB down with `resonance` 0. With `resonance` 1 the peak is 26 dB up at `slope` 12 and 21 dB at `slope` 24, so turn the track down when you use that much.
