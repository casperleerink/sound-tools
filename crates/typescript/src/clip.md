# Clip

Plays its notes once, at its place in the song, out of its output `notes`: a clip of a track a parent of this project (`agent-docs/extensions.md`) makes. Each clip is a record of its own, so moving one clip or editing its notes changes one file:

```json state/song/piano/verse.json
{
  "tool": "clip",
  "state": {
    "start": 3840,
    "length": 3840,
    "notes": [{"start": 0, "length": 960, "pitch": 60, "velocity": 100}, {"start": 960, "length": 960, "pitch": 64, "velocity": 90}]
  }
}
```

| Field | Values | Default |
| --- | --- | --- |
| `start` | where it starts in the song, in ticks: 3840 is a bar of 4/4 | |
| `length` | its length in ticks; a note longer than the rest of the clip ends with it | |
| `notes` | notes that start inside it, in ticks from its start | `[]` |

Its parent connects it to what plays it: `{ from: { child: "verse", notes: "notes" }, to: { child: "piano", notes: "notes" } }`. A note that starts at or after `length` is an error of this record.
