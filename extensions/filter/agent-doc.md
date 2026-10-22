# Filter

`filter` is an effect that lets part of the sound through, with resonance, drive and an LFO on the cutoff.

Here the bass plays through a darker filter named `dark`:

```json state/song/bass/chain/dark.json
{
  "tool": "filter",
  "state": {"type": "low_pass", "cutoff_hz": 600.0}
}
```

{{fields filter}}

Starting points: darker is `low_pass` with `cutoff_hz` 400 to 2000. Thinner, for a sound that fights the bass: `high_pass` at 150 to 400. A telephone voice: `band_pass` at 1500, `resonance` 0.4. A wah: `band_pass` or `low_pass`, `resonance` 0.6, `lfo_rate_hz` 2, `lfo_depth_octaves` 1.5. An acid bass: `low_pass`, `slope` 24, `resonance` 0.7 to 0.9, `"warm"`, some `drive_db`. A sweep over bars is a lane on `cutoff_hz`.
