# EQ: the built-in equaliser effect

`eq` is an effect: it raises or lowers parts of the sound of a track. It has four bands. Each band has a shape, a frequency, a gain and a Q, and can be on or off. After the bands comes one output gain.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the vocal plays through an EQ named `clear`:

```json state/arrangement/vocal/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Vocal",
    "colour": "blue",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["clear"]
  }
}
```

```json state/arrangement/vocal/clear.json
{
  "tool": "eq",
  "state": {
    "bands": [
      {"on": true, "shape": "low_cut", "frequency_hz": 100.0, "gain_db": 0.0, "q": 0.71},
      {"on": true, "shape": "bell", "frequency_hz": 400.0, "gain_db": -3.0, "q": 1.5},
      {"on": true, "shape": "bell", "frequency_hz": 3000.0, "gain_db": 2.0, "q": 1.0},
      {"on": true, "shape": "high_shelf", "frequency_hz": 10000.0, "gain_db": 3.0, "q": 0.71}
    ],
    "output_gain_db": 0.0
  }
}
```

It takes away the rumble under 100 Hz, makes the vocal a little less boxy at 400 Hz, brings it forward at 3 kHz and adds air above 10 kHz.

`bands` is a list of at most four bands: band 1 first, as the card numbers them. A band you leave out, or a field you leave out of a band, takes its default, so `"state": {}` is the default EQ, which leaves the sound exactly as it is. To change one band, write the whole list as it is in the file and change that band.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `on` | Off, the band leaves the sound as it is and keeps its settings. | `true` or `false` | `true` |
| `shape` | What the band does, see below. | `"low_cut"`, `"low_shelf"`, `"bell"`, `"notch"`, `"high_shelf"`, `"high_cut"` | `"low_shelf"`, `"bell"`, `"bell"`, `"high_shelf"` |
| `frequency_hz` | Where the band works. | 20 to 20000 | 100, 400, 2000, 8000 |
| `gain_db` | How much a shelf or a bell raises (above 0) or lowers (below 0). A cut and a notch have no gain. | -15 to 15 | 0 |
| `q` | How narrow a bell or a notch is: 0.5 is wide, 4 is narrow, 18 very narrow. For a cut or a shelf, above 0.71 adds a bump at the frequency, which grows fast: keep it under 2 there. | 0.1 to 18 | 0.71 |
| `output_gain_db` | A gain after all the bands, on `state` and not on a band. | -12 to 12 | 0 |

The shapes:

- `low_cut` takes away what is below the frequency, 12 dB per octave. It is 3 dB down at the frequency with `q` 0.71.
- `high_cut` takes away what is above the frequency, the same way.
- `low_shelf` raises or lowers everything below the frequency by `gain_db`. At the frequency it is half of that.
- `high_shelf` does the same above the frequency.
- `bell` raises or lowers a band around the frequency by `gain_db`, exactly that much at the frequency.
- `notch` takes one narrow band out, completely at the frequency.

Starting points: take the rumble out of almost anything but a bass or a kick with `low_cut` at 80 to 120 Hz. Less boxy is a `bell` at 300 to 500 Hz, -3 dB, `q` 1.5. More presence is a `bell` at 2 to 5 kHz, +2 to +4 dB, `q` 1. More air is a `high_shelf` at 10 kHz, +3 dB. Darker is a `high_shelf` at 4 kHz, -4 dB, or a `high_cut` at 6 to 10 kHz. A hum is a `notch` at 50 or 60 Hz, `q` 8. Small moves of 2 to 4 dB are usually enough; when a boost makes the track louder, bring `output_gain_db` down by about as much.

An edit applies while the track plays and glides over 20 ms, also a change of shape or turning a band on or off, so it does not click.
