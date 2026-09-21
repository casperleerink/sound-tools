# Raw takes: what was played, before any edit

When a composer records a MIDI keyboard, the app writes two things: a clip, which is music you may edit, and a raw take, which is the performance as it arrived.

```text
state/arrangement/piano/take-1.json     the clip: music, in ticks, yours to edit
assets/takes/arrangement/piano/take-1.json   the raw take of that clip: never edit it
```

The path says which clip a take belongs to: it is the id of the clip under `assets/takes/`, with `.json`. So the clip `arrangement/piano/take-1` has the take above.

## Do not edit or delete a take

The app writes a take once, when the recording ends, and never touches it again. Undo of the recording removes the clip and leaves the take. It is the only copy of what the composer played, in real time, and a later step fits the project tempo to it. Read it, copy it, quote numbers from it. Do not change it, move it or remove it. If you want to change the music, change the clip.

A clip with no take next to it was not recorded. That is normal.

## The format

```json assets/takes/arrangement/piano/take-1.json
{
  "clip": "arrangement/piano/take-1",
  "start_tick": 0,
  "end_tick": 3840,
  "events": [
    {"kind":"pedal","time_us":0,"value":127},
    {"kind":"on","time_us":15230,"pitch":60,"velocity":88},
    {"kind":"off","time_us":412870,"pitch":60,"velocity":64},
    {"kind":"pedal","time_us":980400,"value":0}
  ]
}
```

- `clip`: the clip this take was recorded into, by its id.
- `start_tick`, `end_tick`: where the recording began and ended on the project timeline, in ticks. The clip has the same start.
- `events[].time_us`: microseconds from the moment recording began. This is real time as it was played, not musical time. It is the same time however the tempo map changes.
- `kind`: `on` a key went down, `off` a key came up, `pedal` the sustain pedal moved.
- `pitch`: MIDI note number, 0 to 127. `velocity`: 1 to 127 on a key down, 0 to 127 on a key up. A key up velocity is what the keyboard sent; most send 0 or 64.
- `value` of a pedal: 0 to 127 as the pedal was pressed. It counts as down from 64.

The clip holds the same performance in ticks, at the place the app played it, and drops the key up velocity. The take is the only place with the real times.
