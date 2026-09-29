# Modulation: the built-in chorus, flanger and phaser

`modulation` is an effect with three modes. Each moves the sound with an LFO. A `chorus` adds a copy of the sound whose pitch wobbles a little, which makes it wider and thicker. A `flanger` adds a copy only a few ms behind, which makes a comb of notches that sweeps up and down: the jet sound. A `phaser` sweeps three wide notches through the sound: softer and rounder than a flanger. The other fields mean the same in every mode.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the guitar plays through a modulation named `swirl`:

```json state/arrangement/guitar/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Guitar",
    "colour": "teal",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["swirl"]
  }
}
```

```json state/arrangement/guitar/swirl.json
{
  "tool": "modulation",
  "state": {"mode": "chorus", "rate_hz": 0.5, "depth": 0.5, "feedback": 0.2, "spread": 0.5, "mix": 0.5}
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default chorus.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `mode` | What the LFO moves: the delay of a copy around 12 ms, the delay of a copy around 1.5 ms, or six allpass filters around 1 kHz. | `"chorus"`, `"flanger"`, `"phaser"` | `"chorus"` |
| `rate_hz` | How fast the LFO goes up and down. | 0.05 to 10 | 0.5 |
| `depth` | How far the LFO moves. 1 is 7.4 to 19.5 ms for the chorus, 0.27 to 8.5 ms for the flanger, and 250 Hz to 4 kHz for the phaser. 0 holds it still. | 0 to 1 | 0.5 |
| `feedback` | How much of the wet sound goes round again. More makes the notches ring and sound metallic. | 0 to 1 | 0.2 |
| `spread` | How far apart the LFO of the left and the right side is. 0 moves both together, 0.5 is a quarter cycle apart, 1 is half a cycle: opposite, the widest. | 0 to 1 | 0.5 |
| `mix` | 0 is only the sound as it came in, 1 is only the wet sound. The notches of a flanger and a phaser are deepest at 0.5. | 0 to 1 | 0.5 |

Starting points: a wide chorus on a pad or a guitar is `chorus` with `rate_hz` 0.4, `depth` 0.6, `spread` 1. A vibrato is `chorus` with `mix` 1, `feedback` 0, `rate_hz` 5 and `depth` 0.3. A slow jet on drums is `flanger` with `rate_hz` 0.1, `depth` 0.8, `feedback` 0.7. A classic phaser on keys is `phaser` with `rate_hz` 0.3, `depth` 0.7, `feedback` 0.5.

An edit applies while the track plays and glides over 20 ms, so it does not click; `depth` and `spread` glide over 100 ms, so the pitch does not jump. A change of `mode` fades from one to the other over 20 ms. The wet sound keeps about the level of what goes in at every `feedback`, so `mix` means the same at every setting.
