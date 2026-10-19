# Saturator: the built-in saturation effect

`saturator` is an effect: it drives the sound of a track into a curve that rounds its peaks off. A little drive makes a sound warmer and denser, a lot makes it distorted. The level stays where it was as the drive goes up, so the drive changes the colour and not the loudness. A tone tilt after the curve makes the result darker or brighter.

Here the organ plays through a warm saturator named `heat`:

```json state/song/organ/chain/heat.json
{
  "tool": "saturator",
  "state": {"curve": "tube", "drive_db": 9.0}
}
```

{{fields saturator}}

Starting points: warmth on a bass, keys or a voice is `soft` or `tube` with `drive_db` 6 to 12. Glue on a drum bus is `tape` at 6 to 10. Grit is `tube` at 18 to 24 with `tone_db` -3. A clipper that shaves the peaks of drums is `clip` at 3 to 6. Distortion is `clip` or `soft` at 24 to 36, often with `mix` 0.3 to 0.5 so the sound keeps its body.

Every number here can move over time with a `lane` child of its parent (`agent-docs/lane.md`).

With `curve` `clip` and `drive_db` 0 the sound under full scale comes out with nothing added: the only change is a DC blocker at 5 Hz, which takes 0.26 dB off at 20 Hz. The saturator delays the sound by 64 frames, 1.3 ms at 48 kHz, and reports it, so the track stays in time. An edit applies while the track plays and glides over 20 ms, so it does not click.
