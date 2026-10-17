# Sequencer

Loops one of its note patterns at a time in time with the bars, to the sample and in a render too, and sends its notes out of its output `notes`. It is a child of a parent tool of this project (`agent-docs/extensions.md`, "Children"): `state/<parent>/<name>.json`. An example, the bass of a parent `loops`:

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

The parent connects its notes: `{ from: { child: "bass", notes: "notes" }, to: { child: "synth", notes: "notes" } }`.

A launch is a number into its input `launch`: the index of a pattern, or -1 to stop. What played goes on until it lands. A performer of the parent sends it as to a live control, `set({ child: "bass", live: "launch" }, 1)`, from a page, `tick` or `onKey`; a cable sends it, `{ from: { child: "conductor", number: "scene" }, to: { child: "bass", live: "launch" } }`, and launches each time its number changes. Like `play`, a launch is a performance: nothing saves it, and a render does not hear it.

A pattern with a note outside it is silent and a problem of this record.
