# Gate

`gate` is an effect that turns a track down while it is quieter than `threshold_db`, with a transient shaper for the start and tail of each hit. A steady sound passes the shaper unchanged.

Here the snare plays through a gate named `tight`:

```json state/song/snare/chain/tight.json
{
  "tool": "gate",
  "state": {"threshold_db": -30.0, "hold_ms": 15.0, "release_ms": 80.0}
}
```

{{fields gate}}

Its input `sidechain` keys the gate: connect the kick there and a pad opens only while the kick plays. The shaper still follows the sound it plays.

Starting points: a tight snare or tom is `threshold_db` just under the quietest hit, `hold_ms` 10 to 30, `release_ms` 50 to 150. A gentler gate: `range_db` 10 to 20. Punchier drums with no gate: `range_db` 0, `transient_db` 4 to 8. Drier drums: `range_db` 0, `sustain_db` -6 to -12.
