# Arrangement: tracks, clips and notes

The piece is one arrangement that owns tracks. A track owns its clips and one instrument, and it plays to the main output by itself. Adding music never needs a `project.json` edit.

```text
state/arrangement/instance.json              the arrangement
state/arrangement/<track>/instance.json      a track
state/arrangement/<track>/instrument.json    the instrument of the track, always this name
state/arrangement/<track>/<clip>.json        a clip, under any other name
```

A clip only loads inside a track folder, and a track only inside the arrangement folder. Anywhere else the file is listed in `problems.txt` with where it belongs.

To see what plays where, list a track folder and read `start` and `length` of its clips. Open only the clips that overlap the bars you work on.

## A clip: `arrangement.clip`

```json state/arrangement/piano/chords-bars-5-8.json
{
  "tool": "arrangement.clip",
  "state": {
    "start": {{bar_5_start}},
    "length": {{four_bars}},
    "notes": [
      {"start": 0, "length": {{ticks_per_bar}}, "pitch": 48, "velocity": 90},
      {"start": 0, "length": {{ticks_per_bar}}, "pitch": 64, "velocity": 80},
      {"start": 0, "length": {{ticks_per_bar}}, "pitch": 67, "velocity": 80},
      {"start": {{ticks_per_bar}}, "length": {{ticks_per_beat}}, "pitch": 53, "velocity": 90}
    ]
  }
}
```

This clip covers bars 5 to 8. It plays a C chord for the whole of bar 5 and one F on the first beat of bar 6.

- `start`: where the clip starts in the project, in ticks. `length`: how long it is, 1 tick or more.
- `notes[].start` counts from the start of the clip, not of the project: 0 is the first tick of the clip. Every note starts inside the clip, so below the clip `length`. Else the file does not load.
- `notes[].length`: how long the note is held, 1 tick or more. A note that is longer than the rest of the clip stops where the clip ends.
- `pitch`: MIDI note number, 0 to 127, one step per semitone. C4 (middle C) is 60, A4 is 69, C3 is 48, C2 is 36.
- `velocity`: how hard the note is played, 1 to 127. It sets the loudness: 64 is a quarter as loud as 127.
- A chord is several notes with the same `start`. Write one note per line, sorted by `start`.
- Two notes of the same pitch that overlap on one track sound as one: the pitch is held until the last of them ends.
- Clips on one track may overlap in time. The notes of both play.

## A track: `arrangement.track`

```json state/arrangement/piano/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Piano", "colour": "blue", "order": 0}
}
```

- `name`: what the composer sees. Not empty. The folder name is the id and stays as it is when the name changes.
- `colour`: `blue`, `sapphire`, `sky`, `teal`, `green`, `yellow`, `peach`, `red`, `maroon`, `mauve`, `pink`, `lavender`, `rosewater` or `flamingo`. `blue` when left out.
- `order`: tracks show from the lowest to the highest. Tracks with the same order show by id. 0 when left out.
- The track plays through the file `instrument.json` in its folder. Its record is in the doc of the instrument, `agent-docs/instrument.md`. A track without it is silent.

## The arrangement: `arrangement`

```json state/arrangement/instance.json
{
  "tool": "arrangement",
  "state": {}
}
```

It has no settings. Leave it as it is.

## How to

- Add a part: write one new clip file into the folder of the track. Give the clip the bar range of the part, and count the note starts from the clip start.
- Change a part: write its clip file again, whole. Notes that sound are not left hanging.
- Add a track: make a new folder under `state/arrangement/` with `instance.json` first, then `instrument.json`, then its clips. Give it an `order` above the highest one in use and a `colour` no other track has.
- Move a clip in time: change its `start`. Move it to another track: move the file into the folder of that track.
- Delete a clip: remove its file. Delete a track: remove its folder.
- Change the sound of a track: edit its `instrument.json`.
