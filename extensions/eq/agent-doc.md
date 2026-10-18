# EQ: the built-in equaliser effect

`eq` is an effect: it raises or lowers parts of the sound of a track. It has four bands. Each band has a shape, a frequency, a gain and a Q, and can be on or off. After the bands comes one output gain.

Here the vocal plays through an EQ named `clear`:

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

{{fields eq}}

Write the whole list of `bands` every time: to change one band, copy all four from the file and change that one. At its defaults the EQ leaves the sound exactly as it is.

A band:

{{fields eq bands}}

Starting points: take the rumble out of almost anything but a bass or a kick with `low_cut` at 80 to 120 Hz. Less boxy is a `bell` at 300 to 500 Hz, -3 dB, `q` 1.5. More presence is a `bell` at 2 to 5 kHz, +2 to +4 dB, `q` 1. More air is a `high_shelf` at 10 kHz, +3 dB. Darker is a `high_shelf` at 4 kHz, -4 dB, or a `high_cut` at 6 to 10 kHz. A hum is a `notch` at 50 or 60 Hz, `q` 8. Small moves of 2 to 4 dB are usually enough; when a boost makes the track louder, bring `output_gain_db` down by about as much.

Every number here can move over time with an automation lane of the track, named by its path: `bands[0].gain_db` is the gain of band 1. See `agent-docs/arrangement.md`.

An edit applies while the track plays and glides over 20 ms, also a change of shape or turning a band on or off, so it does not click.
