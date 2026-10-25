# Timeline

Plays notes in time, to the sample, out of its output `notes`: once at their place in the song, such as the clips of a track, one record each; or round after round, such as the patterns of a launcher.

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
| `length` | one round, in ticks from `start`: a note that is longer ends there, and none may start at or after it. A loop needs one | as long as its notes |
| `notes` | notes, in ticks from `start` | `[]` |
| `loop` | `true` plays round after round from `start`, `false` once | `false` |
| `plays` | whether it plays from `start` before any launch, so a render plays it | `true` |
| `quantize` | where a launch lands while the piece plays: `"bar"`, `"beat"` or `"now"`. While it is stopped, at once | `"bar"` |
| `pedal` | the sustain pedal: `{"start": 0, "value": 127}` moves in ticks from `start`, down from 64. It goes up where the timeline ends | `[]` |
| `bend`, `mod_wheel`, `pressure` | expression lanes: `{"tick": 0, "value": 8191}` points in ticks from `start`, a straight line between them, held to the end. Bend is -8192 to 8191 (a full bend is two semitones on a sound of this project), the others 0 to 127. Only a timeline that plays once, at its place in the song | `[]` |
| `title` | what a view calls it; nothing plays it | `""` |

The piece ends where the last timeline that plays once stops: a render and an export play to there, and the transport shows that length. A loop and one with `"plays": false` add nothing.

Its parent connects it to what plays it: `{ from: { child: "verse", notes: "notes" }, to: { child: "piano", notes: "notes" } }`.

A number into its input `launch` starts it at its next `quantize` point: above 0 starts it, 0 or below stops it. `send({ child: "bass", number: "launch" }, 1)` is not saved and a render does not hear it; a number cable to `{ child: "bass", number: "launch" }` launches each time its number changes, in a render too. A clip launcher is a set of looping timelines with `"plays": false`: start one and stop the others at the same bar.

A looping timeline shows what it plays to its parent's view: `outputs["bass.playing"]` is 1 while it plays, `queued` is 1 for a start and -1 for a stop that waits, and `phase` goes from 0 to 1 over a round.
