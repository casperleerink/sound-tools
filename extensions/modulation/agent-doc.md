# Modulation: the built-in chorus, flanger and phaser

`modulation` is an effect with three modes. Each moves the sound with an LFO. A `chorus` adds a copy of the sound whose pitch wobbles a little, which makes it wider and thicker. A `flanger` adds a copy only a few ms behind, which makes a comb of notches that sweeps up and down: the jet sound. A `phaser` sweeps three wide notches through the sound: softer and rounder than a flanger. The other fields mean the same in every mode.

Here the guitar plays through a wide chorus named `swirl`:

```json state/song/guitar/chain/swirl.json
{
  "tool": "modulation",
  "state": {"mode": "chorus", "rate_hz": 0.4, "depth": 0.6, "spread": 1.0}
}
```

{{fields modulation}}

Starting points: a wide chorus on a pad or a guitar is the example above. A vibrato is `chorus` with `mix` 1, `feedback` 0, `rate_hz` 5 and `depth` 0.3. A slow jet on drums is `flanger` with `rate_hz` 0.1, `depth` 0.8, `feedback` 0.7. A classic phaser on keys is `phaser` with `rate_hz` 0.3, `depth` 0.7, `feedback` 0.5.

Every number here can move over time with a `lane` child of its parent (`agent-docs/lane.md`).

An edit applies while the track plays and glides over 20 ms, so it does not click; `rate_hz`, `depth` and `spread` glide over 100 ms, so the pitch does not jump. A change of `mode` fades from one to the other over 20 ms. The wet sound keeps about the level of what goes in at every `feedback`, so `mix` means the same at every setting.
