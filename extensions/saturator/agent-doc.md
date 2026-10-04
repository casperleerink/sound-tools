# Saturator: the built-in saturation effect

`saturator` is an effect: it drives the sound of a track into a curve that rounds its peaks off. A little drive makes a sound warmer and denser, a lot makes it distorted. The level stays where it was as the drive goes up, so the drive changes the colour and not the loudness. A tone tilt after the curve makes the result darker or brighter.

Here the organ plays through a saturator named `heat`:

```json state/arrangement/organ/heat.json
{
  "tool": "saturator",
  "state": {"curve": "soft", "drive_db": 6.0, "tone_db": 0.0, "output_db": 0.0, "mix": 1.0}
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default saturator.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `curve` | The shape. `soft` rounds off evenly. `tape` is gentler and never flattens. `tube` leans to one side and adds even harmonics, the warmest. `clip` is clean up to a ceiling and then flat: the drive lowers the ceiling. | `"soft"`, `"tape"`, `"tube"`, `"clip"` | `"soft"` |
| `drive_db` | How hard the sound goes into the curve. The output is turned down by what the drive adds to a sine at -12 dBFS, so the level stays. | 0 to 36 | 6 |
| `tone_db` | A tilt after the curve, around 1 kHz. Above 0 the highs go up and the lows down by half of it each: brighter. Below 0 darker. | -12 to 12 | 0 |
| `output_db` | Gain on the saturated sound. | -12 to 12 | 0 |
| `mix` | 1 is only the saturated sound, 0 is the sound as it came in. Between is parallel saturation. | 0 to 1 | 1 |

Starting points: warmth on a bass, keys or a voice is `soft` or `tube` with `drive_db` 6 to 12. Glue on a drum bus is `tape` at 6 to 10. Grit is `tube` at 18 to 24 with `tone_db` -3. A clipper that shaves the peaks of drums is `clip` at 3 to 6. Distortion is `clip` or `soft` at 24 to 36, often with `mix` 0.3 to 0.5 so the sound keeps its body.

Every number here can move over time with an automation lane of the track: see `agent-docs/arrangement.md`.

With `curve` `clip` and `drive_db` 0 the sound under full scale comes out with nothing added: the only change is a DC blocker at 5 Hz, which takes 0.26 dB off at 20 Hz. The saturator delays the sound by 64 frames, 1.3 ms at 48 kHz, and reports it, so the track stays in time. An edit applies while the track plays and glides over 20 ms, so it does not click.
