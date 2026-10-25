# Modulation

`modulation` is an effect with three modes, chorus, flanger and phaser, each moved by an LFO. The other fields mean the same in every mode.

Here the guitar plays through a wide chorus named `swirl`:

```json state/song/guitar/chain/swirl.json
{
  "tool": "modulation",
  "state": {"mode": "chorus", "rate_hz": 0.4, "depth": 0.6, "spread": 1.0}
}
```

{{fields modulation}}

Starting points: a wide chorus on a pad or a guitar is the example. A vibrato: `chorus`, `mix` 1, `feedback` 0, `rate_hz` 5, `depth` 0.3. A slow jet on drums: `flanger`, `rate_hz` 0.1, `depth` 0.8, `feedback` 0.7. A phaser on keys: `phaser`, `rate_hz` 0.3, `depth` 0.7, `feedback` 0.5.
