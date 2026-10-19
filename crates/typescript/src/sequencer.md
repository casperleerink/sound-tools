# Sequencer

Loops one of its note patterns at a time, in time with the bars and to the sample, out of its output `notes`. A child of a parent of this project (`agent-docs/extensions.md`), such as the bass of `loops`:

```json state/loops/bass.json
{
  "tool": "sequencer",
  "state": {
    "patterns": [
      {"length": 3840, "notes": [{"start": 0, "length": 480, "pitch": 36, "velocity": 100}, {"start": 1920, "length": 480, "pitch": 43, "velocity": 100}]},
      {"length": 7680, "notes": [{"start": 0, "length": 7680, "pitch": 31, "velocity": 90}]}
    ],
    "plays": 0
  }
}
```

| Field | Values | Default |
| --- | --- | --- |
| `patterns` | a list of `{"length", "notes"}`: its length in ticks, 3840 a bar of 4/4, and notes that start inside it, in ticks from its start | `[]` |
| `plays` | the pattern it plays before any launch, by its index from 0, so a render plays it; a change launches it | none |
| `quantize` | where a launch lands while the piece plays: `"bar"`, `"beat"` or `"now"`. While it is stopped, at once | `"bar"` |

A number into its input `launch` launches: the index of a pattern, or -1 or one it does not have to stop; what plays goes on until it lands. From a performer, `send({ child: "bass", number: "launch" }, 1)`, nothing saves it and a render does not hear it. A number cable to `{ child: "bass", number: "launch" }` launches each time its number changes, in a render too.

A pattern with a note outside it is silent and a problem of this record.
