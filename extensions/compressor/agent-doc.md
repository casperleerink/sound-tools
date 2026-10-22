# Compressor

`compressor` is an effect that turns a track down while it is louder than `threshold_db`.

Here the drums play through a compressor named `glue`, for punchier drums:

```json state/song/drums/chain/glue.json
{
  "tool": "compressor",
  "state": {"ratio": 5.0, "attack_ms": 30.0, "release_ms": 80.0}
}
```

{{fields compressor}}

`lookahead_ms` cannot move with a lane.

Its input `sidechain` keys it: connect the kick there and the bass ducks while the kick plays. A lookahead does not make it duck before the key.

Starting points: an even vocal or bass is `ratio` 3 to 4, `attack_ms` 10 to 20, `release_ms` 100 to 200, `threshold_db` where the loud notes are, `makeup_db` about half the reduction. Punchier drums: the example. Tamed peaks: `ratio` 10 to 20, `attack_ms` 0.5 to 2, `knee_db` 0, `lookahead_ms` 1. Parallel: `ratio` 8, `threshold_db` -35, `mix` 0.4, some `makeup_db`. On deep bass keep `release_ms` at 100 or more, or it distorts.
