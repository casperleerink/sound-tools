# Utility: the built-in utility effect

`utility` is an effect for the plain jobs of a mix: gain, pan, stereo width, the bass in mono, which channels play, a channel turned upside down, and mute. At its defaults it changes nothing, to the bit, so it is safe anywhere in a chain. Use it to turn a sound down before an effect that is driven too hard, to narrow a pad or make it wider, to keep the bass of a wide sound in the middle, or to fix a recording whose left and right cancel.

Here the pad plays through a utility named `narrow` that makes it narrower:

```json state/arrangement/pad/narrow.json
{
  "tool": "utility",
  "state": {"width": 0.6}
}
```

{{fields utility}}

Inside, `channels` and the inverts come first, then `width`, then `bass_mono`, and last `gain_db`, `pan` and `mute`.

Starting points: a pad that takes too much room is `width` 0.6, as above. A wider synth is `width` 1.5 with `bass_mono` on, so its low end stays in the middle. A mono recording that is only in the left channel is `channels` `"left"`. A stereo recording that goes thin in mono often has one channel upside down: `invert_right` `true`. Before an effect that is driven too hard, `gain_db` -6 gives it room.

Every number here can move over time with an automation lane of the track: see `agent-docs/arrangement.md`.

An edit applies while the track plays and glides over 20 ms, so it does not click. Turning `bass_mono` on or off fades between the sound as it came and the sound through the crossover over those 20 ms, and the bass near `bass_mono_hz` dips for that moment.
