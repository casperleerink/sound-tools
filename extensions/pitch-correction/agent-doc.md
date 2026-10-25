# Pitch correction

`pitch-correction` is an effect that moves one voice, or one melodic instrument, to the nearest note of its `key` and `scale`. It goes on the voice's audio track. The length of the sound stays, and with `keep_formants` the tone of the voice too.

Here the vocal is hard tuned to A minor:

```json state/song/vocal/chain/tune.json
{
  "tool": "pitch-correction",
  "state": {"key": "A", "scale": "minor", "retune_ms": 0.0}
}
```

{{fields pitch-correction}}

Notes held on its `notes` input are the notes it moves to, in any octave, in place of the key and scale. To have a clip lead the voice, connect it in `project.json`: `{"from": {"instance": "song/keys/verse", "port": "notes"}, "to": {"input": {"instance": "song/vocal/chain/tune", "port": "notes"}}}`.

Starting points: a natural correction is `retune_ms` 50 to 150, hard tuning `retune_ms` 0. Long notes that should keep their vibrato: `humanize` 0.5 to 1. Use the key of the song: a wrong key tunes the voice to wrong notes. Chords, drums and noise pass through unchanged. The sound comes out 38 ms late; the project makes up for it.
