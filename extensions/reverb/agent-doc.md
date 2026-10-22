# Reverb

`reverb` is an effect that puts the sound in a room.

Here the keys play through a small room named `room`:

```json state/song/keys/chain/room.json
{
  "tool": "reverb",
  "state": {"size": 0.3, "decay_seconds": 0.8, "pre_delay_ms": 5.0}
}
```

{{fields reverb}}

A change of `size` while `freeze` is on waits until freeze is off.

Starting points: a hall is `size` 0.9, `decay_seconds` 3.5, `pre_delay_ms` 30. A dark plate on a voice: `size` 0.6, `decay_seconds` 1.8, `damping` 0.7, `high_cut_hz` 5000. A wash behind a pad: `decay_seconds` 12, `size` 1, `mix` 0.5. On a bass or a kick keep `low_cut_hz` at 200 or more, or use no reverb.
