# Raw takes

When a `recorder` records notes (`agent-docs/recorder.md`), the app writes a `timeline` you may edit and a raw take: the performance as it arrived.

```text
state/song/piano/rec-take-1.json    the timeline: music, in ticks, yours to edit
assets/takes/take-1.json            the raw take it came from: never edit it
```

## Do not edit or delete a take

It is the only copy of what the composer played, in real time, and undo leaves it. Read it, copy it, quote it. Never change, move or remove it; to change the music, change the timeline.

## The format

```json assets/takes/take-1.json
{
  "start_us": 0,
  "end_us": 2000000,
  "start_tick": 0,
  "end_tick": 3840,
  "pedal_at_start": 0,
  "events": [
    {"kind":"pedal","time_us":0,"sounded_us":0,"value":127},
    {"kind":"on","time_us":15230,"sounded_us":16000,"pitch":60,"velocity":88},
    {"kind":"bend","time_us":300100,"sounded_us":301333,"value":2048},
    {"kind":"off","time_us":412870,"sounded_us":413333,"pitch":60,"velocity":64},
    {"kind":"pedal","time_us":980400,"sounded_us":981333,"value":0}
  ]
}
```

- `start_us`, `end_us`: where the recording began and ended, in microseconds from the start of the piece, whatever the tempo map does.
- `start_tick`, `end_tick`: the same moments in ticks under the tempo map of then. Only to read: a tempo change makes them wrong.
- `pedal_at_start`: the sustain pedal at the start, 0 to 127.
- A wheel or the pressure away from rest at the start has a message at `time_us` 0.
- `events[].time_us`: microseconds from the start of the recording, when the message reached the app: the performance as played.
- `events[].sounded_us`: when the engine sounded it, up to about 1.5 ms later. A note put here renders what the composer heard.
- `kind`: `on`, `off`, `pedal` (sustain), `bend`, `mod_wheel` or `pressure`.
- `pitch`: MIDI note, 0 to 127. `velocity`: 1 to 127 on `on`, 0 to 127 on `off`.
- `value` of a pedal: 0 to 127, down from 64. Of a bend: -8192 to 8191, 0 in the middle. Of a mod wheel or pressure: 0 to 127.

The timeline drops the key up velocity and thins the wheel lanes; the take keeps the real times and every move.
