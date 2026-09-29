# Limiter: the built-in limiter effect

`limiter` is an effect: it keeps a track or a bus under its ceiling. A peak that would go over is turned down at once, just enough, and the gain comes back with the release. Push the sound in with `gain_db` and the track gets louder while its peaks stay at the ceiling. It is the same limiter as the one at the end of the master, as a device you can put anywhere.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the drums play through a limiter named `peaks`:

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
    "effects": ["peaks"]
  }
}
```

```json state/arrangement/drums/peaks.json
{
  "tool": "limiter",
  "state": {
    "gain_db": 0.0,
    "ceiling_db": -1.0,
    "release_ms": 100.0,
    "lookahead_ms": 1
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default limiter.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `gain_db` | How much louder the sound goes into the limiter. Every dB over the ceiling is taken off again, so this makes the track louder without raising its peaks. | 0 to 24 | 0 |
| `ceiling_db` | The highest a sample may reach, in dBFS. Under it the sound passes untouched. | -24 to 0 | -1 |
| `release_ms` | How fast the gain comes back after a peak: the time to 63 % of the way. Short is louder and can distort bass; long is cleaner and can pump. | 10 to 1000 | 100 |
| `lookahead_ms` | How far ahead it looks, so it turns down before a peak arrives and the top of the wave keeps its shape. It delays the track by as much, and the app keeps the track in time. `0` adds no delay and clips the first edge of a peak. | `0`, `1` or `5` | `1` |

Starting points: catch the odd peak of a vocal or a bass with the defaults. A louder drum bus is `gain_db` 3 to 6 and `release_ms` 50. For a louder mix, put it last on a bus, with `gain_db` a few dB and `release_ms` 100 to 300. Several dB of reduction all the time sound squashed; take `gain_db` down until it only moves on the loudest hits.

It holds the samples under the ceiling, not the wave between them, which can go over it once the sound is played or made into a lossy file: most for high tones. The default of -1 dB leaves room for it. A change of `gain_db` glides over 20 ms, so it does not click.
