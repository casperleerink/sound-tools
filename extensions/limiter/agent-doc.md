# Limiter: the built-in limiter effect

`limiter` is an effect: nothing it sends out goes over its ceiling. The ceiling holds where the limiter is in `effects`: effects after it, and the pan of the track, can take the sound over it again, so put it last for a track that must stay under it. A peak that would go over is turned down at once, just enough, and the gain comes back with the release. Push the sound in with `gain_db` and the track gets louder while its peaks stay at the ceiling. It is the same limiter as the one at the end of the master, as a device you can put anywhere.

Here the loops play through a limiter named `peaks`, which makes them 4 dB louder:

```json state/arrangement/loops/peaks.json
{
  "tool": "limiter",
  "state": {"gain_db": 4.0}
}
```

{{fields limiter}}

Starting points: catch the odd peak of a vocal or a bass with the defaults. Louder drums are `gain_db` 3 to 6 and `release_ms` 50. To make a whole track louder, put it last in `effects`, with `gain_db` a few dB and `release_ms` 100 to 300. For the whole mix, use the limiter of the master in `agent-docs/arrangement.md` instead. Several dB of reduction all the time sound squashed; take `gain_db` down until it only moves on the loudest hits.

`gain_db` and `release_ms` can move over time with an automation lane of the track: see `agent-docs/arrangement.md`. `ceiling_db` cannot, because a new ceiling applies at once and a lane would step it.

It holds the samples under the ceiling, not the wave between them, which can go over it once the sound is played or made into a lossy file: most for high tones. The default of -1 dB leaves room for it. A change of `gain_db` glides over 20 ms, so it does not click.
