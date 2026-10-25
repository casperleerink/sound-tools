# Limiter

`limiter` is an effect: nothing it sends out goes over `ceiling_db`. Effects after it, and a pan, can take the sound over again, so put it last for a sound that must stay under. Push the sound in with `gain_db` to make it louder with its peaks at the ceiling.

Here the loops play through a limiter named `peaks`, which makes them 4 dB louder:

```json state/song/loops/chain/peaks.json
{
  "tool": "limiter",
  "state": {"gain_db": 4.0}
}
```

{{fields limiter}}

Only `gain_db` and `release_ms` can move with a lane.

It holds the samples under the ceiling, not the wave between them, so a played or lossy file can go a little over. Keep `ceiling_db` at -1 or lower when that matters.

Starting points: catch the odd peak with the defaults. Louder drums: `gain_db` 3 to 6, `release_ms` 50. A louder track or mix: last in the chain, `gain_db` a few dB, `release_ms` 100 to 300. Lower `gain_db` until it only moves on the loudest hits, or it sounds squashed.
