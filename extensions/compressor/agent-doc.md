# Compressor: the built-in compressor effect

`compressor` is an effect: it turns a track down when it gets louder than the threshold, so loud and quiet parts sit closer together. A gentle setting makes a part even and steady; a fast attack tames the peaks of drums; a slow attack lets the start of each note through, which makes it punchier; makeup gain brings the level back up after it.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the drums play through a compressor named `glue`:

```json state/arrangement/drums/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Drums",
    "colour": "sky",
    "order": 2,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["glue"]
  }
}
```

```json state/arrangement/drums/glue.json
{
  "tool": "compressor",
  "state": {
    "threshold_db": -18.0,
    "ratio": 4.0,
    "attack_ms": 10.0,
    "release_ms": 120.0,
    "knee_db": 6.0,
    "makeup_db": 0.0,
    "mix": 1.0,
    "lookahead_ms": 0
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default compressor.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `threshold_db` | The peak level in dBFS where it starts to turn down. Lower compresses more of the sound. | -60 to 0 | -18 |
| `ratio` | How much it turns down above the threshold. 4 means 4 dB over the threshold comes out 1 dB over. 1 does nothing, 100 is a limiter. | 1 to 100 | 4 |
| `attack_ms` | How fast it turns down when the sound gets louder: the time to 63 % of the way. | 0.1 to 300 | 10 |
| `release_ms` | How fast it lets go when the sound gets quieter: the time to 63 % of the way, after the loud part has passed. | 1 to 3000 | 120 |
| `knee_db` | How gently it starts: the width of the bend around the threshold. 0 is a hard corner. | 0 to 18 | 6 |
| `makeup_db` | Gain after the compression, to bring the level back up. | 0 to 24 | 0 |
| `mix` | 1 is only the compressed sound, 0 is the sound as it came in. Between is parallel compression. | 0 to 1 | 1 |
| `lookahead_ms` | How far ahead it listens, so it can turn down before a peak arrives. It delays the track by as much, and the app keeps the track in time. | `0`, `1` or `10` | `0` |

Starting points: an even vocal or bass is `ratio` 3 to 4, `attack_ms` 10 to 20, `release_ms` 100 to 200, with `threshold_db` where the loud notes are, then `makeup_db` about half the reduction. Punchier drums: `ratio` 4 to 6, `attack_ms` 20 to 40, `release_ms` 60 to 120. Tamed peaks: `ratio` 10 to 20, `attack_ms` 0.5 to 2, `knee_db` 0, `lookahead_ms` 1. Parallel compression: `ratio` 8, `threshold_db` -35, `mix` 0.4, some `makeup_db`.

The level is the peak of both channels over the last 10 ms. Under 50 Hz that makes the gain move with the wave and adds harmonics, most with a short release: for a 25 Hz tone 14 dB over the threshold at `ratio` 4, they are 35 dB under the tone at `release_ms` 10 and 54 dB under at 120. For deep bass keep `release_ms` at 100 or more. For a steady sound whose peak is 12 dB over the threshold at `ratio` 4, the sound comes out 9 dB quieter, plus `makeup_db`. An edit applies while the track plays and glides over 20 ms, so it does not click.
