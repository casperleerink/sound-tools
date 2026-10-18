# Compressor: the built-in compressor effect

`compressor` is an effect: it turns a track down when it gets louder than the threshold, so loud and quiet parts sit closer together. A gentle setting makes a part even and steady; a fast attack tames the peaks of drums; a slow attack lets the start of each note through, which makes it punchier; makeup gain brings the level back up after it.

Here the drums play through a compressor named `glue`, for punchier drums:

```json state/arrangement/drums/glue.json
{
  "tool": "compressor",
  "state": {"ratio": 5.0, "attack_ms": 30.0, "release_ms": 80.0}
}
```

{{fields compressor}}

Starting points: an even vocal or bass is `ratio` 3 to 4, `attack_ms` 10 to 20, `release_ms` 100 to 200, with `threshold_db` where the loud notes are, then `makeup_db` about half the reduction. Punchier drums: `ratio` 4 to 6, `attack_ms` 20 to 40, `release_ms` 60 to 120, as in the example. Tamed peaks: `ratio` 10 to 20, `attack_ms` 0.5 to 2, `knee_db` 0, `lookahead_ms` 1. Parallel compression: `ratio` 8, `threshold_db` -35, `mix` 0.4, some `makeup_db`.

Every number here but `lookahead_ms` can move over time with an automation lane of the track: see `agent-docs/arrangement.md`.

Another track can key it, so the bass ducks while the kick plays: see the sidechain in `agent-docs/arrangement.md`. A lookahead does not make it duck before the key: it hears the key as it plays.

The level is the peak of both channels over the last 10 ms. Under 50 Hz that makes the gain move with the wave and adds harmonics, most with a short release: for a 25 Hz tone 14 dB over the threshold at `ratio` 4, they are 35 dB under the tone at `release_ms` 10 and 54 dB under at 120. For deep bass keep `release_ms` at 100 or more. For a steady sound whose peak is 12 dB over the threshold at `ratio` 4, the sound comes out 9 dB quieter, plus `makeup_db`. An edit applies while the track plays and glides over 20 ms, so it does not click.
