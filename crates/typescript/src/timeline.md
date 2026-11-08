# Timeline

Plays notes in time out of its output `notes`: once at their place in the song, such as the clips of a track, one record each; or round after round, such as the patterns of a launcher.

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
| `length` | one round, in ticks from `start`: a longer note ends there, and none may start at or after it. A loop needs one | as long as its notes |
| `notes` | notes, in ticks from `start` | `[]` |
| `loop` | `true` plays round after round from `start` | `false` |
| `plays` | whether it plays from `start` before any launch, so a render plays it | `true` |
| `quantize` | where a launch lands while the piece plays: `"bar"`, `"beat"` or `"now"`. While it is stopped, at once | `"bar"` |
| `pedal` | the sustain pedal: `{"start": 0, "value": 127}` moves in ticks from `start`, down from 64. It goes up where the timeline ends | `[]` |
| `bend`, `mod_wheel`, `pressure` | `{"tick": 0, "value": 8191}` points in ticks from `start`, a straight line between them. Bend is -8192 to 8191 (a full bend is two semitones on a sound of this project), the others 0 to 127 | `[]` |
| `controllers` | other MIDI controllers by number, not 1 or 64: `{"74": [{"tick": 0, "value": 64}]}`, as `mod_wheel`, but each plays from its first point and sends nothing after the end: the instrument keeps what it heard last, also after a seek. Plugins hear them, the bundled instruments do not | `{}` |
| `title` | what a view calls it | `""` |

The lanes need a timeline that plays once: one that loops or waits for a launch plays notes only.

The piece ends where the last timeline that plays once, or the last region, stops: a render and an export play to there. A loop and one with `"plays": false` do not count.

Its number input `launch` starts it at the next `quantize` point when above 0, and stops it there at 0 or below. `send({ child: "bass", number: "launch" }, 1)` is not saved and a render does not hear it; a number cable into `launch` is heard in a render too. A clip launcher is a set of looping timelines with `"plays": false`: start one and stop the others at the same bar.

A looping timeline shows its parent's view `outputs["bass.playing"]`, 1 while it plays; `queued`, 1 for a start and -1 for a stop that waits; and `phase`, 0 to 1 over a round.
