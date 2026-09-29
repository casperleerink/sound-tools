# Utility: the built-in utility effect

`utility` is an effect for the plain jobs of a mix: gain, pan, stereo width, the bass in mono, which channels play, a channel turned upside down, and mute. At its defaults it changes nothing, to the bit, so it is safe anywhere in a chain. Use it to turn a sound down before an effect that is driven too hard, to narrow a pad or make it wider, to keep the bass of a wide sound in the middle, or to fix a recording whose left and right cancel.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the pad plays through a utility named `narrow`:

```json state/arrangement/pad/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Pad",
    "colour": "lavender",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["narrow"]
  }
}
```

```json state/arrangement/pad/narrow.json
{
  "tool": "utility",
  "state": {
    "gain_db": 0.0,
    "pan": 0.0,
    "width": 1.0,
    "bass_mono": false,
    "bass_mono_hz": 120.0,
    "channels": "stereo",
    "invert_left": false,
    "invert_right": false,
    "mute": false
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default utility.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `gain_db` | Gain in dB. | -36 to 36 | 0 |
| `pan` | -1 is left, 1 is right, with the pan law of a track: a sound keeps its loudness wherever it is. Hard left, the left channel is 3 dB up and the right one is silent. | -1 to 1 | 0 |
| `width` | 0 is mono, 1 is the sound as it came, 2 is twice as wide. It scales the difference of the two channels and keeps their sum. | 0 to 2 | 1 |
| `bass_mono` | `true` makes the sound below `bass_mono_hz` mono and leaves the rest as wide as it was. The two bands add up flat. | `true` or `false` | `false` |
| `bass_mono_hz` | Where bass mono starts, in Hz. | 50 to 500 | 120 |
| `channels` | Which channels play: `"stereo"` as they are, `"left"` or `"right"` that channel in both, `"swap"` left and right swapped. | `"stereo"`, `"left"`, `"right"`, `"swap"` | `"stereo"` |
| `invert_left` | `true` turns the left channel upside down: its polarity, what a phase button flips. | `true` or `false` | `false` |
| `invert_right` | The same for the right channel. | `true` or `false` | `false` |
| `mute` | `true` silences what comes out of this utility. To mute the whole track, use `mute` of the track. | `true` or `false` | `false` |

Inside, `channels` and the inverts come first, then `width`, then `bass_mono`, and last `gain_db`, `pan` and `mute`.

Starting points: a pad that takes too much room is `width` 0.6. A wider synth is `width` 1.5 with `bass_mono` on, so its low end stays in the middle. A mono recording that is only in the left channel is `channels` `"left"`. A stereo recording that goes thin in mono often has one channel upside down: `invert_right` `true`. Before an effect that is driven too hard, `gain_db` -6 gives it room.

An edit applies while the track plays and glides over 20 ms, so it does not click. Turning `bass_mono` on or off fades between the sound as it came and the sound through the crossover over those 20 ms, and the bass near `bass_mono_hz` dips for that moment.
