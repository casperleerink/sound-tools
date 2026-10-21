# Gate: the built-in gate and transient shaper

`gate` is an effect: it turns a track down while it is quieter than the threshold, so the bleed and the ring between drum hits go away. Its transient shaper makes the start of each hit louder or softer, and its tail longer or shorter, whatever the level.

Here the snare plays through a gate named `tight`:

```json state/song/snare/chain/tight.json
{
  "tool": "gate",
  "state": {"threshold_db": -30.0, "hold_ms": 15.0, "release_ms": 80.0}
}
```

{{fields gate}}

Starting points: a tight snare or tom is `threshold_db` just under the quietest hit, `hold_ms` 10 to 30, `release_ms` 50 to 150. A gentler gate keeps some room with `range_db` 10 to 20. Punchier drums with no gate: `range_db` 0, `transient_db` 4 to 8. Drier drums: `range_db` 0, `sustain_db` -6 to -12.

Every number here can move over time with a `lane` child of its parent (`agent-docs/lane.md`).

Its input `sidechain` keys it: connect the kick there and a pad opens only while the kick plays. The gate then follows the key; the transient shaper always follows the sound it plays.

The level is the peak of both channels over the last 10 ms, so the gate closes `hold_ms` after the sound stayed under the threshold for 10 to 11 ms. The transient is about the first 20 ms of a hit, the tail what falls after it; a steady sound passes the shaper unchanged.
