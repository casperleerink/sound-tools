# Recorder

Records into the song while its number input `record` is above 0 and the song plays: a child of a parent of this project (`agent-docs/extensions.md`). What it heard becomes children of its parent, next to it, where it was heard, in one undo step:

- the notes that reach its input `notes` become a `timeline` (`agent-docs/timeline.md`) named `<recorder>-take-<n>`, and the performance is kept under `assets/takes/take-<n>.json`;
- the channels of the audio input that `channels` names become a `region` (`agent-docs/region.md`) of a new file, `assets/audio/<recorder>-sound-<n>.wav`, placed by when each frame was captured.

```json state/song/voice/rec.json
{"tool": "recorder", "state": {"channels": [0]}}
```

| Field | Values | Default |
| --- | --- | --- |
| `channels` | the channels of the audio input it records, from 0: `[]` for notes alone, `[0]` mono, `[0, 1]` stereo | `[]` |

A take ends when `record` goes to 0, when the song stops, or when it jumps, such as a loop going back: then the next take starts there. A change of the tempo ends a take too. One recording of the audio input runs at a time, for every recorder that started with it. While `record` is on, also before the song plays, the audio input is open, so the composer sees the level and the take has its sound from the first frame. Turn it on from a view, as an arm button: `send({ child: "rec", number: "record" }, 1)`; nothing saves it. Its parent connects what it records: `{ from: { notes: "notes" }, to: { child: "rec", notes: "notes" } }` records the keyboard. While it records, `now.take("rec")` in a `Wave` draws the sound so far.
