# Utility

`utility` is an effect for gain, pan, width, mono bass, channel routing, polarity and mute.

Here the pad plays through a utility named `narrow` that makes it narrower:

```json state/song/pad/chain/narrow.json
{
  "tool": "utility",
  "state": {"width": 0.6}
}
```

{{fields utility}}

Starting points: a wider synth is `width` 1.5 with `bass_mono` on. A mono recording only in the left channel is `channels` `"left"`. A stereo recording that goes thin in mono often has one channel upside down: `invert_right` `true`. Before an effect driven too hard, `gain_db` -6.
