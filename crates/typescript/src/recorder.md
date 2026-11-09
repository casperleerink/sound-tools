# Recorder

Records into the song while its number input `record` is above 0 and the song plays. Each take reaches the parent's `onTake` as the record of a child, placed where it was heard, for the parent to keep with `update`:

- the notes that reach its input `notes` as a `timeline` named `<recorder>-take-<n>`, with the pedal and the wheels; the performance is kept under `assets/takes/take-<n>.json`;
- the channels of the audio input that `channels` names as a `region` of a new file, `assets/audio/<recorder>-sound-<n>.wav`.

```json state/song/voice/rec.json
{"tool": "recorder", "state": {"channels": [0]}}
```

| Field | Values | Default |
| --- | --- | --- |
| `channels` | the channels of the audio input it records, from 0: `[]` for notes alone, `[0]` mono, `[0, 1]` stereo | `[]` |

A take ends when `record` goes to 0, when the song stops or jumps (a loop going back starts the next take), or when the tempo changes. While its number input `arm` or `record` is above 0 and `channels` names one, the audio input is open, so the composer sees the level. A view sends both, `send({ child: "rec", number: "arm" }, 1)`, and nothing saves either. `{ from: { notes: "notes" }, to: { child: "rec", notes: "notes" } }` records the keyboard. While it records, `now.take("rec")` draws the take so far.
