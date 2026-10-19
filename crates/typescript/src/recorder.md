# Recorder

Records into the song while its number input `record` is above 0 and the song plays: a child of a parent of this project (`agent-docs/extensions.md`). Each take reaches the parent's `onTake` as the record of a child to keep it, placed where it was heard; the parent keeps it with `update`, such as a new child `{ name: take.name, tool: take.tool, state: take.state }`:

- the notes that reach its input `notes` as a `timeline` (`agent-docs/timeline.md`) named `<recorder>-take-<n>`, with the pedal and the wheels; the performance is kept under `assets/takes/take-<n>.json`;
- the channels of the audio input that `channels` names as a `region` (`agent-docs/region.md`) of a new file, `assets/audio/<recorder>-sound-<n>.wav`, placed by when each frame was captured.

A parent with no `onTake` keeps only the files.

```json state/song/voice/rec.json
{"tool": "recorder", "state": {"channels": [0]}}
```

| Field | Values | Default |
| --- | --- | --- |
| `channels` | the channels of the audio input it records, from 0: `[]` for notes alone, `[0]` mono, `[0, 1]` stereo | `[]` |

A take ends when `record` goes to 0, when the song stops, or when it jumps, such as a loop going back: then the next take starts there. A change of the tempo ends a take too. One recording of the audio input runs at a time, for every recorder that started with it; one that starts meanwhile, as at a loop, starts once it is done. While its number input `arm` or `record` is on, also before the song plays, the audio input is open, so the composer sees the level and the take has its sound from the first frame. An arm button turns `arm` on, and a record button `record`, from a view: `send({ child: "rec", number: "arm" }, 1)`; nothing saves either. Its parent connects what it records: `{ from: { notes: "notes" }, to: { child: "rec", notes: "notes" } }` records the keyboard. While it records, `now.take("rec")` in a `Wave` draws the sound so far.
