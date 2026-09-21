# Raw takes: what was played, before any edit

When a composer records a MIDI keyboard, the app writes two things: a clip, which is music you may edit, and a raw take, which is the performance as it arrived.

```text
state/arrangement/piano/take.json    the clip: music, in ticks, yours to edit
assets/takes/take-1.json             the raw take it came from: never edit it
```

The clip says which take it came from, in its `take` field:

```text
{"tool": "arrangement.clip", "state": {"start": 0, "length": 3840, "notes": [], "take": "take-1"}}
```

`"take": "take-1"` means the file `assets/takes/take-1.json`. It is a plain reference, like every other link in a project that is not ownership. It is a field and not the path of the clip, because a clip moves: to another track, or to another name. **Keep the field as it is** when you change a clip, move its file or copy it. A clip with no `take` was not recorded, which is normal, and a clip whose take file is gone still plays.

Take names are `take-1`, `take-2` and so on, and a name is never used twice in one project. Two clips never share a take.

## Do not edit or delete a take

The app writes a take once, when the recording ends, and never opens the file again. Undo of the recording removes the clip and leaves the take. It is the only copy of what the composer played, in real time, and a later step fits the project tempo to it. Read it, copy it, quote numbers from it. Do not change it, move it or remove it. If you want to change the music, change the clip.

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
    {"kind":"off","time_us":412870,"sounded_us":413333,"pitch":60,"velocity":64},
    {"kind":"pedal","time_us":980400,"sounded_us":981333,"value":0}
  ]
}
```

- `start_us`, `end_us`: where the recording began and ended on the project timeline, in microseconds from the start of the piece. This is real time and it means the same whatever the tempo map does, which is why fitting the tempo can keep the take where it was heard.
- `start_tick`, `end_tick`: the same two moments in ticks, under the tempo map of that moment. They are there to read. Do not compute from them: a tempo change makes them mean something else.
- `pedal_at_start`: how far the sustain pedal was already pressed when the recording began, 0 to 127.
- `events[].time_us`: microseconds from the moment recording began, taken when the message reached the app. This is the performance as it was played. The beat finder of a tempo fit works from these.
- `events[].sounded_us`: microseconds from the same moment, when the engine really sounded the message, which is the start of the audio block that carried it, up to about 1.5 ms later. A clip that puts a note here renders what the composer heard.
- `kind`: `on` a key went down, `off` a key came up, `pedal` the sustain pedal moved.
- `pitch`: MIDI note number, 0 to 127. `velocity`: 1 to 127 on a key down, 0 to 127 on a key up. A key up velocity is what the keyboard sent; most send 0 or 64.
- `value` of a pedal: 0 to 127 as the pedal was pressed. It counts as down from 64.

The clip holds the same performance in ticks, at the place the app played it, and drops the key up velocity. The take is the only place with the real times.

A take written before the app saved `start_us`, `end_us` and `sounded_us` does not load. Nothing plays worse for it: only a tempo fit reads a take, and it says so.
