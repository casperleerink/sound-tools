# Timeline

Plays notes once at their positions in the song, out of its output `notes`, as the transport passes them: a child of a parent of this project (`agent-docs/extensions.md`). A parent may have many, each a record of its own, such as the clips of a track: moving one or editing its notes changes one file.

```json state/song/piano/verse.json
{
  "tool": "timeline",
  "state": {
    "start": 3840,
    "length": 3840,
    "notes": [{"start": 0, "length": 960, "pitch": 60, "velocity": 100}, {"start": 960, "length": 960, "pitch": 64, "velocity": 90}]
  }
}
```

| Field | Values | Default |
| --- | --- | --- |
| `start` | where its notes count from in the song, in ticks: 3840 is a bar of 4/4 | `0` |
| `length` | where it ends, in ticks from `start`: a note that is longer ends there, and none may start at or after it | none: every note plays to its end |
| `notes` | notes, in ticks from `start` | `[]` |

Its parent connects it to what plays it: `{ from: { child: "verse", notes: "notes" }, to: { child: "piano", notes: "notes" } }`.
