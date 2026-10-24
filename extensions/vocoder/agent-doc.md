# Vocoder

`vocoder` is an effect that gives the sound of its track the shape of another sound, band by band, so a synth speaks or sings with a voice. It goes on the synth's track; the voice keys it through its `sidechain` input. With nothing there it follows its own sound.

Here the pad plays through a vocoder named `talk`:

```json state/song/pad/chain/talk.json
{
  "tool": "vocoder",
  "state": {"bands": 32, "release_ms": 40.0}
}
```

{{fields vocoder}}

Connect the voice track to `sidechain`. The synth should be bright and held, such as a saw chord: the vocoder only shapes what the synth already plays. The sound comes out 21 ms late; the project makes up for it.

Starting points: clear speech is `bands` 24 to 40 and `release_ms` 30 to 80. A robot is `bands` 8 to 12. `formant` 3 to 5 makes a smaller voice, -3 to -5 a bigger one.
