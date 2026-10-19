# Reverb: the built-in reverb effect

`reverb` is an effect: it puts the sound of a track in a room. A short decay and a small size is a small room, a long decay and a large size a hall. The pre-delay is the gap before the room answers. Freeze holds the tail as it is, for a pad that never ends.

Here the keys play through a small room named `room`:

```json state/song/keys/room.json
{
  "tool": "reverb",
  "state": {"size": 0.3, "decay_seconds": 0.8, "pre_delay_ms": 5.0}
}
```

{{fields reverb}}

Starting points: a hall is `size` 0.9, `decay_seconds` 3.5, `pre_delay_ms` 30. A dark plate on a voice is `size` 0.6, `decay_seconds` 1.8, `damping` 0.7, `high_cut_hz` 5000. A wash behind a pad is `decay_seconds` 12, `size` 1, `mix` 0.5. On a bass or a kick drum keep `low_cut_hz` at 200 or more, or leave the reverb off.

Every number here can move over time with a `lane` child of its parent (`agent-docs/lane.md`); `size` and `pre_delay_ms` follow their lane in 20 ms fades from one room to the next.

An edit applies while the track plays and glides over 20 ms, so it does not click. A change of `size` or `pre_delay_ms` fades from the old room to the new one over those 20 ms. The tail comes out at about the level of what goes in, whatever `size` and `decay_seconds` are, so `mix` means the same at every setting. Turn `freeze` on while a chord rings to hold it; turn it off and the tail dies away in the decay time. A change of `size` while frozen waits until freeze is off.
