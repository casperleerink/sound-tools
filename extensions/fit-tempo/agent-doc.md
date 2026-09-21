# Fit the tempo to a take, and correct a fit that is wrong

A composer plays freely, with no click. One action fits the project's grid to that performance: the tempo map follows the playing, and the take sounds exactly as it did. What an algorithm cannot know is in one small record, and that is what you correct.

```text
state/fit-tempo.json                 the fit: the only file you edit
project.json                         tempo_map: written by the fit, do not edit
state/arrangement/piano/take.json    the clip of the take: written by the fit, do not edit
assets/takes/take-1.json             the performance: never edited by anyone
```

Change `state/fit-tempo.json` and the tempo map and the clip are made again from it at once, as one undo step. Nothing else has to be touched, and a value you write is never overwritten.

## The record

```json state/fit-tempo.json
{
  "tool": "fit-tempo",
  "state": {
    "take": "take-1",
    "first_downbeat_us": 0,
    "beat": "normal",
    "steadiness": 0.0
  }
}
```

- `take`: the file `assets/takes/<take>.json`. The clip that has the same name in its own `take` field is the one the fit writes the notes into.
- `first_downbeat_us`: which moment of the take is beat 1 of a bar, in microseconds from the start of the recording, the unit the take file uses. The beat nearest to it becomes the first downbeat, and it lands on a bar line. `0` means the first beat that was found.
- `beat`: `half`, `normal` or `double`. How many beats the grid has for each beat that was found.
- `steadiness`: 0 for the tempo as it was played, 1 for one steady tempo. Anything between moves the beats towards even spacing. The notes keep their ticks whatever it is, so a piece can always be turned back to 0.

The file lives at the top of `state/`. One project has one fit; a second `fit-tempo` record changes nothing and says so in `problems.txt`.

## When the grid is wrong

**The grid runs twice as fast as the music.** Every beat of the grid falls on half a beat of the music. Set `"beat": "normal"` if it says `double`, or `"half"` if it says `normal`.

**The grid runs half as fast as the music.** The other way: `"normal"` if it says `half`, or `"double"` if it says `normal`.

**The bar lines are in the wrong place.** The beats are right and beat 1 falls where the music has beat 2, 3 or 4. Move `first_downbeat_us` to the moment the music really begins a bar. Open `assets/takes/<take>.json`, find the note on there and copy its `time_us`:

```text
{"kind":"on","time_us":1436000,"sounded_us":1436645,"pitch":48,"velocity":84}
```

Then write `"first_downbeat_us": 1436000`. It does not have to be exact: the beat nearest to it is taken.

**The bars are the wrong length.** The time signature is the project's, not the fit's. Change `time_signature` in `project.json`, for example from `"4/4"` to `"3/4"`, and the grid is built again for it in the same undo step. The tempo changes in that file are written by the fit; leave them alone.

**The tempo wobbles more than the playing did.** Raise `steadiness`. `0.3` takes some of it out, `1.0` leaves one tempo.

## What a correction costs

A correction makes the clip's notes again from the raw take, so it is exact however many times you correct it. **Edits made by hand to that clip before a correction are lost.** One undo brings them back, together with the tempo map and the fit record, because the three are one step. Add parts to other clips, not to the take's clip.

## Parts that follow the take

After a fit, bars and beats mean what the composer played. A clip you write at bar 5 starts where the fifth bar of the performance starts, whatever the tempo does there. So the ordinary way of adding a part works: write the clip at the bar you want, with `start` and note starts in ticks, as `agent-docs/arrangement.md` says. Nothing about a fitted project is special for you.

`runtime . --inspect` prints the fit in one line: which take, the beat, the steadiness, how many beats the grid has and where the first downbeat is.

## What the beat finder gets wrong

It is a deterministic algorithm, not a model, and it is right about the beat within about 30 ms on ordinary playing. Three things it cannot know from the timing alone, which are the three fields above: whether a beat is a beat or half of one, where a bar begins, and how many beats a bar has. It is also weaker right at a sudden change of tempo, where one beat may land a fifth of a beat out, and it needs at least eight chords or notes to find anything at all.

## Check your work

`problems.txt` in the project folder lists everything that is not live, including what a fit could not do: a take file that is missing, a take with too little in it, and a clip that no longer names the take. A missing file means no runtime is watching and nothing has checked your edit.
