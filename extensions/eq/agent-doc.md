# EQ

`eq` is an effect with four bands and an output gain.

Here the vocal plays through an EQ named `clear`:

```json state/song/vocal/chain/clear.json
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

{{fields eq}}

Write the whole list of `bands` every time: to change one band, copy all four from the file and change that one.

A band:

{{fields eq bands}}

A lane names a band number by its path: `bands[0].gain_db` is the gain of band 1.

Starting points: rumble out (not on a bass or kick) is `low_cut` at 80 to 120 Hz. Less boxy: `bell` at 300 to 500 Hz, -3 dB, `q` 1.5. Presence: `bell` at 2 to 5 kHz, +2 to +4 dB, `q` 1. Air: `high_shelf` at 10 kHz, +3 dB. Darker: `high_shelf` at 4 kHz, -4 dB, or `high_cut` at 6 to 10 kHz. Hum: `notch` at 50 or 60 Hz, `q` 8. Moves of 2 to 4 dB are usually enough; after a boost, lower `output_gain_db` by about as much.
