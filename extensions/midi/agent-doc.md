# Raw takes: what was played, before any edit

When a `recorder` records notes (`agent-docs/recorder.md`), the app writes two things: a `timeline`, which is music you may edit, and a raw take, which is the performance as it arrived.

```text
state/song/piano/rec-take-1.json    the timeline: music, in ticks, yours to edit
assets/takes/take-1.json            the raw take it came from: never edit it
```

Take names are `take-1`, `take-2` and so on, and a name is never used twice in one project.

## Do not edit or delete a take

The app writes a take once, when the recording ends, and never opens the file again. Undo of the recording removes the timeline and leaves the take. It is the only copy of what the composer played, in real time. Read it, copy it, quote numbers from it. Do not change it, move it or remove it. If you want to change the music, change the timeline.

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

- `start_us`, `end_us`: where the recording began and ended on the project timeline, in microseconds from the start of the piece. This is real time and it means the same whatever the tempo map does.
- `start_tick`, `end_tick`: the same two moments in ticks, under the tempo map of that moment. They are there to read. Do not compute from them: a tempo change makes them mean something else.
- `pedal_at_start`: how far the sustain pedal was already pressed when the recording began, 0 to 127.
- A wheel or the pressure that was away from rest when the recording began has a message at `time_us` 0. Without one it was at rest.
- `events[].time_us`: microseconds from the moment recording began, taken when the message reached the app. This is the performance as it was played.
- `events[].sounded_us`: microseconds from the same moment, when the engine really sounded the message, which is the start of the audio block that carried it, up to about 1.5 ms later. A note put here renders what the composer heard.
- `kind`: `on` a key went down, `off` a key came up, `pedal` the sustain pedal moved, `bend` the bend wheel moved, `mod_wheel` the modulation wheel moved, `pressure` the keys were pressed harder or softer.
- `pitch`: MIDI note number, 0 to 127. `velocity`: 1 to 127 on a key down, 0 to 127 on a key up. A key up velocity is what the keyboard sent; most send 0 or 64.
- `value` of a pedal: 0 to 127 as the pedal was pressed. It counts as down from 64. Of a bend: -8192 to 8191, 0 in the middle. Of a mod wheel or pressure: 0 to 127.

The timeline holds the same performance in ticks, at the place the app played it, and drops the key up velocity. It keeps the wheels as lanes with only the points a straight line cannot replace. The take is the only place with the real times and every move.

A take written before the app saved `start_us`, `end_us` and `sounded_us` does not load. Nothing plays worse for it: no tool reads a take.
