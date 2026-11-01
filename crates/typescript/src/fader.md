# Fader

Turns a sound up or down on a fader's scale, pans it and mutes it, from its input `audio` to its output `audio`: a mixer strip, a send or a master volume.

```json state/song/bass/mix.json
{"tool": "fader", "state": {"gain_db": -6, "pan": -0.3}}
```

| Field | Values | Default |
| --- | --- | --- |
| `gain_db` | decibels up to 6, or `"-inf"` for silence | `0` |
| `pan` | -1 hard left, 0 the middle, 1 hard right; a sound keeps its loudness wherever it is | `0` |
| `mute` | silent | `false` |

A cable sets `gain_db` in decibels and `pan`, each held in its range, and `mute`: 1 is silent, 0 plays: `{ from: { child: "volume", number: "value" }, to: { child: "mix", number: "gain_db" } }`. A view reads the level of what it sends on with `now.peak("mix")`, as of a `meter`.

A send is a fader too: its parent connects the sound to it before or after the strip's fader, and its output to the input of a return.
