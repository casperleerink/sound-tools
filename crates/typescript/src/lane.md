# Lane

Moves a number over the song, out of its output `value`: automation, a child of a parent of this project (`agent-docs/extensions.md`). Its parent connects it to a number of another child, and that number follows the line, to the sample, while the song plays and while it stands still.

```json state/song/bass/lane-cutoff.json
{
  "tool": "lane",
  "state": {
    "points": [{"tick": 0, "value": 400}, {"tick": 7680, "value": 4000}],
    "scale": "log"
  }
}
```

| Field | Values | Default |
| --- | --- | --- |
| `points` | `{ "tick", "value" }` in ticks from the start of the song (3840 is a bar of 4/4), in tick order, one per tick, at least one, in the units of the number it moves. Before the first point it holds the first value, after the last the last | |
| `scale` | `"linear"`: a straight line between two points. `"log"`: equal ratios in equal times, as a frequency or a time sounds even; its values are above 0 | `"linear"` |

`{ from: { child: "lane-cutoff", number: "value" }, to: { child: "filter", number: "cutoff_hz" } }` moves the filter's cutoff. A value outside the number's range is held at its end. Without the connection the number goes back to its record. A view shows what it plays with `now.value("filter.cutoff_hz", { played: true })`.
