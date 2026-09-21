# Arrangement: tracks, clips and notes

The piece is one arrangement that owns tracks. A track owns its clips and one instrument, and it plays to the main output by itself. Adding music never needs a `project.json` edit.

```text
state/arrangement/instance.json              the arrangement
state/arrangement/<track>/instance.json      a track
state/arrangement/<track>/instrument.json    the instrument of the track, always this name
state/arrangement/<track>/<effect>.json      an effect, named in the track record
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

## A recorded clip: the sustain pedal

A clip the composer recorded from a keyboard has a `pedal` list as well. Leave it out of a clip you write by hand, and keep it when you change one that has it.

```json state/arrangement/piano/take-1.json
{
  "tool": "arrangement.clip",
  "state": {
    "start": 0,
    "length": {{four_bars}},
    "notes": [
      {"start": 0, "length": 900, "pitch": 60, "velocity": 88},
      {"start": 940, "length": 880, "pitch": 64, "velocity": 76}
    ],
    "pedal": [{"start": 0, "value": 127}, {"start": 1880, "value": 0}],
    "take": "take-1"
  }
}
```

- `pedal[].start`: where the pedal moved, in ticks from the start of the clip, like a note start. It must be inside the clip.
- `pedal[].value`: how far the pedal was pressed, 0 to 127, as it was played. It counts as down from 64, and a piano that knows half pedal can use the rest.
- While the pedal is down a note goes on sounding after its own end, until the pedal comes up. So the notes above sound together until tick 1880.
- The pedal is not shown in the note editor yet. Edit it here.
- `take`: the raw take this clip was recorded from, the file `assets/takes/take-1.json`. Keep the field as it is when you change the clip, move its file or copy it: it is the only way back to what the composer played. Read `agent-docs/takes.md` before you touch anything under `assets/takes/`.

## A track: `arrangement.track`

```json state/arrangement/piano/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Piano",
    "colour": "blue",
    "order": 0,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["warmth"]
  }
}
```

- `name`: what the composer sees. Not empty. The folder name is the id and stays as it is when the name changes.
- `colour`: `blue`, `sapphire`, `sky`, `teal`, `green`, `yellow`, `peach`, `red`, `maroon`, `mauve`, `pink`, `lavender`, `rosewater` or `flamingo`. `blue` when left out.
- `order`: tracks show from the lowest to the highest. Tracks with the same order show by id. 0 when left out.
- `gain_db`: how much louder or quieter the track plays, in decibels, -60 to 6. 0 when left out, which is the sound as the instrument makes it. -6 halves the samples, 6 doubles them, and -60 is as quiet as it goes; for silence use `mute`. Change the sound itself in `instrument.json`; change the balance between tracks here.
- `pan`: where the track sits between the two channels, -1 to 1. -1 is hard left, 0 the middle, 1 hard right. 0 when left out. A track keeps its loudness wherever it is panned.
- `mute`: `true` silences the track and changes nothing else. `false` when left out.
- `effects`: the effects of the track, by file name without `.json`, in the order the sound goes through them. Left out when the track has none, and a track that leaves it out is written back without it.
- The track plays through the file `instrument.json` in its folder. Its record is in the doc of the instrument, `agent-docs/instrument.md`. A track without it is silent.

## The effects of a track

The sound of a track goes through its instrument, then through each effect in `effects` in that order, then through `gain_db`, `pan` and `mute`. Two lines make one effect: the record in the track folder, and its name in the list.

```json state/arrangement/piano/warmth.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.warmth", "state_asset": "warmth"}
}
```

An effect is any tool with an `audio` input and an `audio` output. Today that is the `plugin` tool, which is the same record as an instrument; `agent-docs/plugins.md` says where the ids come from. The file name is yours: lowercase letters, digits, `-` and `_`, and not `instrument`.

How to:

- **Add an effect**: write its record into the track folder, then put its file name at the end of `effects` in `instance.json`. Write the record first: a name in the list with no record behind it is reported until the file is there.
- **Reorder**: write `effects` in the order you want. Nothing else moves, and the sound changes at once. `["warmth", "space"]` is the instrument, then warmth, then space.
- **Remove**: take the name out of `effects` and delete the file. Taking it out of the list alone leaves a record that is reported; deleting the file alone leaves a name that is reported.
- **Turn one off for a while**: take its name out of `effects` and leave the file where it is. The record and the plugin's own settings stay, and putting the name back brings it back.

What `problems.txt` says about this, and what to do:

- `` `effects` names "space", and this track has no space.json ``: write that record, or take the name out of the list. The track plays through the rest of the chain meanwhile.
- `` the child "space" takes audio in and makes audio out ``, and the list does not name it: the record is there and nothing goes through it. Add its name to `effects` where you want it, or delete the file.
- `effects[1] is "warmth", which the list already has`, or a name with a capital letter, or `instrument`: the record itself does not load, so the whole track keeps what it had. Correct the list.

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
- Change the sound of a track: edit its `instrument.json`. Put an effect after it, or take one off, with `effects` in `instance.json` and the record next to it.
- Balance the tracks: set `gain_db` in `instance.json` of each. Put a track to one side with `pan`, and silence one with `"mute": true`. All three apply while the project plays.
