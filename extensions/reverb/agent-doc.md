# Reverb: the built-in reverb effect

`reverb` is an effect: it puts the sound of a track in a room. A short decay and a small size is a small room, a long decay and a large size a hall. The pre-delay is the gap before the room answers. Freeze holds the tail as it is, for a pad that never ends.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the keys play through a reverb named `room`:

```json state/arrangement/keys/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Keys",
    "colour": "lavender",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["room"]
  }
}
```

```json state/arrangement/keys/room.json
{
  "tool": "reverb",
  "state": {
    "pre_delay_ms": 20.0,
    "decay_seconds": 2.0,
    "size": 0.5,
    "damping": 0.5,
    "diffusion": 0.7,
    "low_cut_hz": 100.0,
    "high_cut_hz": 8000.0,
    "width": 1.0,
    "mix": 0.3,
    "freeze": false
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default reverb.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `pre_delay_ms` | The gap between the sound and the first reflections, in ms. Longer keeps a voice or a drum clear of its room. | 0.5 to 250 | 20 |
| `decay_seconds` | How long the tail takes to fall by 60 dB, in seconds. | 0.2 to 60 | 2 |
| `size` | How large the room is: how far apart the early reflections are. 0 is a small box and sounds metallic, 1 is a hall. | 0 to 1 | 0.5 |
| `damping` | How much sooner the highs die than the rest. 0 is as long, 0.5 is 55 % of the decay, 1 is a tenth. | 0 to 1 | 0.5 |
| `diffusion` | How quickly the reflections blur into a smooth tail. 0 leaves separate echoes, 1 is smooth at once. | 0 to 1 | 0.7 |
| `low_cut_hz` | The sound that goes into the reverb is cut below this. Raise it to keep the bass out of the room. | 20 to 20000 | 100 |
| `high_cut_hz` | The sound that goes into the reverb is cut above this. Lower it for a darker room. | 20 to 20000 | 8000 |
| `width` | 0 is a mono tail, 1 is as wide as it gets. The dry sound keeps its own width. | 0 to 1 | 1 |
| `mix` | 0 is only the sound as it came in, 1 is only the reverb. | 0 to 1 | 0.3 |
| `freeze` | `true` holds the tail for as long as it is on, lets no new sound into it, and passes the dry sound as before. | `true` or `false` | `false` |

Starting points: a small room is `size` 0.3, `decay_seconds` 0.8, `pre_delay_ms` 5. A hall is `size` 0.9, `decay_seconds` 3.5, `pre_delay_ms` 30. A dark plate on a voice is `size` 0.6, `decay_seconds` 1.8, `damping` 0.7, `high_cut_hz` 5000. A wash behind a pad is `decay_seconds` 12, `size` 1, `mix` 0.5. On a bass or a kick drum keep `low_cut_hz` at 200 or more, or leave the reverb off.

An edit applies while the track plays and glides over 20 ms, so it does not click. A change of `size` or `pre_delay_ms` fades from the old room to the new one over those 20 ms. The loudness of the tail grows with `decay_seconds`, as in a real room, so a long decay may want a lower `mix`. Turn `freeze` on while a chord rings to hold it; turn it off and the tail dies away in the decay time.
