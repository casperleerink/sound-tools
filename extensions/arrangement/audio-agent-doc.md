# Audio: audio tracks and audio clips

An audio track plays stretches of audio files, where an instrument track plays notes. It has no instrument; its effects, `gain_db`, `pan`, `mute` and `solo` are those of any track, see `agent-docs/arrangement.md`.

```text
assets/audio/<file>.wav                    an audio file of the project
state/arrangement/<track>/instance.json    an audio track: "kind": "audio"
state/arrangement/<track>/<clip>.json      an audio clip of that track
```

## The files

- A clip plays a file in `assets/audio/` and nothing else. To use a file from anywhere else, copy it there first. The app never changes a file in that folder.
- The name is lowercase letters, digits, `-` and `_`, a dot and the extension: `voice-take-1.wav`. Rename the copy when the original has capitals or spaces.
- WAV and AIFF, at any sample rate and bit depth, mono or stereo. A file at another sample rate than the device plays at its own pitch and speed.
- Copy the file first, then write the clip. A clip whose file is not there yet is listed in `problems.txt` and is silent; it plays as soon as the file is there.

## An audio track

```json state/arrangement/voice/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Voice",
    "kind": "audio",
    "colour": "peach",
    "order": 3,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false
  }
}
```

- `kind`: `"audio"` makes an audio track. It is chosen when the track is made; an instrument track leaves the field out. Do not change it on a track that has clips.
- `input`: which channels of the composer's audio input the track records when they record in the window: one channel, `[1]`, or two next to each other for a stereo take, `[1, 2]`. Channels count from 1. Leave it out for `[1]`, as this example does.
- No `instrument.json`: an audio track plays its clips through its effects, if any, into the master.

## An audio clip: `arrangement.audio_clip`

```json state/arrangement/voice/verse-take.json
{
  "tool": "arrangement.audio_clip",
  "state": {
    "asset": "voice-take-1.wav",
    "start": 15360,
    "file_start_seconds": 0.5,
    "file_end_seconds": 4.5,
    "gain_db": -3.0,
    "fade_in_ms": 10.0,
    "fade_out_ms": 200.0,
    "layer": 0
  }
}
```

In 4/4 this clip starts on bar 5. It plays four seconds of `assets/audio/voice-take-1.wav`, from 0.5 s to 4.5 s into the file, 3 dB quieter, with a short fade in and a longer fade out.

- `asset`: the file name under `assets/audio/`.
- `start`: where the clip starts in the project, in ticks, like a note clip.
- There is no `length`. A clip plays at the speed of its file, so it lasts as long as the part of the file it plays, in seconds. A tempo change moves where it starts and never how fast it plays. At a tempo of `bpm`, a second is `bpm / 60 * 960` ticks.
- `file_start_seconds`: where in the file the clip starts playing. 0 is the beginning of the file, and 0 when left out.
- `file_end_seconds`: where in the file it stops, after `file_start_seconds`. Leave it out to play to the end of the file. A value past the end of the file stops at the end.
- `gain_db`: how much louder or quieter than the file, a number up to 24, or `"-inf"` for silence. 0 when left out.
- `fade_in_ms`, `fade_out_ms`: a straight line from silence at the start of the clip up to its level, and down to silence at its end. 0 when left out. Every edge of a clip also gets a short ramp of 2 ms that is not a setting, so a clip never clicks; a fade of 0 is that ramp alone. Where one clip hands over to another, the two cross over those 2 ms, so a file cut in two clips that touch plays as it did whole.
- `layer`: where clips of one track overlap, only the one with the higher layer is heard there. The one below is not changed: where the top one ends, it plays on. Of two clips with the same layer the one that starts later is heard. 0 when left out.

## How to

- **Add a clip from a file**: copy the file into `assets/audio/` under a name as above, then write the clip into the folder of an audio track, with `start` where it should play. Give it a `layer` above every clip of the track it overlaps, so it is heard there.
- **Add an audio track**: a new folder under `state/arrangement/` with `instance.json` as above. Give it an `order` above the highest one in use and a `colour` no other track has.
- **Move a clip in time**: change its `start`. Move it to another audio track: move the file into the folder of that track.
- **Trim**: change `file_start_seconds` or `file_end_seconds`. To keep the rest of the clip where it was when you trim its start, move `start` later by the same time in ticks.
- **Louder, quieter, fades**: `gain_db`, `fade_in_ms`, `fade_out_ms`. They apply while the project plays.
- **Delete a clip**: remove its file. The audio file stays in `assets/audio/`.
- **A recorded take** is a file such as `assets/audio/voice-take-1.wav` and a clip of it where the composer heard it. Its `file_start_seconds` skips what the file holds from before the recording began, so leave it as it is unless you mean to trim.
- **Find how long a file is**: `sound-tools . --inspect` prints each audio clip with its file and how long that file is, see `agent-docs/inspect.md`.

What `problems.txt` says, and what to do:

- `plays assets/audio/voice-take-1.wav, which is not there`: copy the file there under that name, or correct `asset`. The clip keeps its place and the rest plays.
- `is silent: ... it is not a WAV or AIFF file`: the file cannot be played. Use a WAV or AIFF file.
- `plays nothing: file_start_seconds ... is at or past the end`: the file is shorter than you thought. Lower `file_start_seconds`.
- `is a note clip, and this is an audio track`: note clips belong in an instrument track, and audio clips in an audio track.
