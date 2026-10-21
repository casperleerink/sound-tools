# Filter: the built-in filter effect

`filter` is an effect: it takes the sound of a track and lets part of it through. Low pass makes a sound darker, high pass thinner, band pass leaves a band in the middle, notch takes one out. Resonance adds a peak at the cutoff, drive warms the sound up before the filter, a warm character makes the filter react to level, and an LFO can move the cutoff up and down.

Here the bass plays through a darker filter named `dark`:

```json state/song/bass/chain/dark.json
{
  "tool": "filter",
  "state": {"type": "low_pass", "cutoff_hz": 600.0}
}
```

{{fields filter}}

Starting points: darker is `low_pass` with `cutoff_hz` 400 to 2000. Thinner, for a sound that fights the bass, is `high_pass` at 150 to 400. A telephone voice is `band_pass` at 1500 with `resonance` 0.4. A wah is `band_pass` or `low_pass` with `resonance` 0.6, `lfo_rate_hz` 2 and `lfo_depth_octaves` 1.5. An acid bass is `low_pass`, `slope` 24, `resonance` 0.7 to 0.9, `"warm"` and some `drive_db`.

A sweep over time, such as a filter that opens over eight bars, is a `lane` child of its parent that moves `cutoff_hz` or any other number here (`agent-docs/lane.md`).

An edit applies while the track plays and glides over 20 ms, so it does not click. At the cutoff a `low_pass` or `high_pass` is 3 dB down with `resonance` 0. With `resonance` 1 the peak is about 12 dB up at `slope` 12 and 9 dB at `slope` 24, and the sound below the peak gets quieter as resonance rises, as on an analog filter.
