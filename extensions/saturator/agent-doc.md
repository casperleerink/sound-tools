# Saturator

`saturator` is an effect that drives the sound into a curve: warmer and denser with a little drive, distorted with a lot. The level stays the same as the drive goes up.

Here the organ plays through a warm saturator named `heat`:

```json state/song/organ/chain/heat.json
{
  "tool": "saturator",
  "state": {"curve": "tube", "drive_db": 9.0}
}
```

{{fields saturator}}

Starting points: warmth on a bass, keys or a voice is `soft` or `tube` with `drive_db` 6 to 12. Glue on a drum bus: `tape` at 6 to 10. Grit: `tube` at 18 to 24, `tone_db` -3. Shaved drum peaks: `clip` at 3 to 6. Distortion: `clip` or `soft` at 24 to 36, often `mix` 0.3 to 0.5.
