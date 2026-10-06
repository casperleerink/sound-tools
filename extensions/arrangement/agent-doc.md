# Arrangement: tracks, clips and notes

The piece is one arrangement that owns tracks. A track owns its clips and one instrument, and it plays into the master of the arrangement by itself, which plays to the main output through a limiter. Adding music never needs a `project.json` edit.

This doc is about instrument tracks, which play notes. An audio track plays audio files instead and has no instrument: open `agent-docs/audio.md` for it and its clips. Everything here about `gain_db`, `pan`, `mute`, `solo` and `effects` holds for both.

```text
state/arrangement/instance.json              the arrangement
state/arrangement/<track>/instance.json      a track
state/arrangement/<track>/instrument.json    the instrument of the track, always this name
state/arrangement/<track>/<effect>.json      an effect, named in the track record
state/arrangement/<track>/<clip>.json        a clip, under any other name
```

A clip only loads inside a track folder, and a track only inside the arrangement folder.

To see what plays where, list a track folder and read `start` and `length` of its clips. Open only the clips that overlap the bars you work on.

## A clip: `arrangement.clip`

```json state/arrangement/piano/intro.json
{
  "tool": "arrangement.clip",
  "state": {
    "start": 15360,
    "length": 15360,
    "notes": [
      {"start": 0, "length": 3840, "pitch": 48, "velocity": 90},
      {"start": 0, "length": 3840, "pitch": 64, "velocity": 80},
      {"start": 0, "length": 3840, "pitch": 67, "velocity": 80},
      {"start": 3840, "length": 960, "pitch": 53, "velocity": 90}
    ]
  }
}
```

In 4/4 this clip covers bars 5 to 8. It plays a C chord for the whole of bar 5 and one F on the first beat of bar 6.

- `start`: where the clip starts in the project, in ticks. `length`: how long it is, 1 tick or more.
- `notes[].start` counts from the start of the clip, not of the project: 0 is the first tick of the clip. It must be below the clip `length`, else the file does not load.
- `notes[].length`: how long the note is held, 1 tick or more. A note stops where the clip ends.
- `pitch`: MIDI note number, 0 to 127, one step per semitone. C4 (middle C) is 60, A4 is 69, C3 is 48, C2 is 36.
- `velocity`: how hard the note is played, 1 to 127. It sets the loudness: 64 is a quarter as loud as 127.
- A chord is several notes with the same `start`. Write one note per line, sorted by `start`.
- Two notes of the same pitch that overlap on one track sound as one: the pitch is held until the last of them ends.
- Clips on one track may overlap in time. The notes of both play.
- A clip the composer recorded also has `pedal`, the sustain pedal, and `take`, the raw take it came from. Keep both as they are when you change, move or copy the clip, and leave `pedal` out of a clip you write. Read `agent-docs/takes.md` before you touch anything under `assets/takes/`.

## Bend, mod wheel and pressure: the lanes of a clip

A clip may move the bend wheel, the modulation wheel and the key pressure of its instrument, each with a lane of points.

```json state/arrangement/lead/slide.json
{
  "tool": "arrangement.clip",
  "state": {
    "start": 0,
    "length": 3840,
    "notes": [{"start": 0, "length": 3840, "pitch": 64, "velocity": 90}],
    "bend": [
      {"tick": 0, "value": 0},
      {"tick": 480, "value": 8191},
      {"tick": 1920, "value": 8191},
      {"tick": 2400, "value": 0}
    ],
    "mod_wheel": [{"tick": 1920, "value": 0}, {"tick": 3839, "value": 100}]
  }
}
```

This note slides up over an eighth, stays up, and comes back down in the third beat, while a vibrato grows over the second half of the bar.

- `bend`, `mod_wheel`, `pressure`: each a list of points. Leave out a lane you do not use.
- `tick`: from the start of the clip, like a note start, below its `length`. The points of a lane are in tick order, at most one per tick, else the file does not load.
- `value` of a bend: -8192 to 8191, 0 in the middle. The built-in synth and Sampler bend two semitones either way, so a semitone up is 4096. Of a mod wheel or pressure: 0 to 127. The synth and the Sampler add a vibrato with the mod wheel; no built-in instrument uses the pressure.
- Between two points the value moves in a straight line. Before the first point it holds the first value, after the last the last. So a sudden move is two points a tick apart.
- Outside its clips a lane is at 0, and it goes back there when its clip ends.
- When clips on one track overlap and both have points in one lane, the clip that starts later is heard in that lane.

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
- `order`: tracks show from the lowest to the highest, and those with the same order by id. 0 when left out.
- `gain_db`: the volume of the track in decibels: a number up to 6, or `"-inf"` for silence. 0 when left out. -6 halves the samples, 6 doubles them. Balance the tracks here; change the sound itself in `instrument.json`.
- `pan`: -1 to 1. -1 is hard left, 0 the middle, 1 hard right. 0 when left out. A track keeps its loudness wherever it is panned.
- `mute`: `true` silences the track. `false` when left out.
- `solo`: while any track has `"solo": true`, only the soloed tracks play. A muted track stays silent when it is soloed. Left out when off.
- `effects`: the effects of the track, see below. Left out when the track has none.
- The track plays through `instrument.json` in its folder, whose record is in `agent-docs/instrument.md`. A track without it is silent.

A new track is a folder under `state/arrangement/` with `instance.json` first, then `instrument.json`, then its clips. Give it an `order` above the highest one in use and a `colour` no other track has.

## The effects of a track

The sound of a track goes through its instrument, then through each effect in `effects` in that order, then through `gain_db`, `pan`, `mute` and `solo`, into the master. One effect is two things: its record in the track folder, and its file name in `effects`. To add one, write the record first, then put its name in the list where you want it; to remove one, take the name out and delete the file. Either half alone is reported in `problems.txt`.

```json state/arrangement/piano/warmth.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.warmth", "state_asset": "warmth"}
}
```

An effect is any tool with an `audio` input and an `audio` output. Today that is the built-in `filter`, `compressor`, `limiter`, `eq`, `delay`, `reverb`, `saturator`, `utility` and `modulation`, each with its record in `agent-docs/<tool>.md`, and the `plugin` tool, which is the same record as an instrument; `agent-docs/plugins.md` says where the ids come from. The file name is yours: lowercase letters, digits, `-` and `_`, and not `instrument`.

To turn an effect off for a while, write its slot as `{"name": "warmth", "bypass": true}`, and as `"warmth"` again to turn it on. The sound goes past it untouched, without its latency, and its record stays.

An effect with a `sidechain` input, such as the `compressor`, can follow the sound of another track instead of its own: the bass ducks while the kick plays. Its slot says which track keys it. Here the drums key the compressor `duck` of the sub bass, which turns the bass down on every hit:

```json state/arrangement/sub/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Sub",
    "colour": "red",
    "order": 4,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": [{"name": "duck", "sidechain": {"track": "drums", "tap": "post_fx"}}]
  }
}
```

```json state/arrangement/sub/duck.json
{
  "tool": "compressor",
  "state": {"threshold_db": -30.0, "ratio": 8.0, "attack_ms": 1.0, "release_ms": 150.0}
}
```

- `track`: the folder name of the track that keys it. It may be the same track.
- `tap`: where its sound is taken. `pre_fx`: what its instrument or clips play, before its effects. `post_fx`: after its effects, before its volume, pan, mute and solo. `post_mixer`: what it sends to the master.
- A track silenced by its mute or by the solo of another track keys nothing with `post_mixer`. Use `post_fx` to keep the key while you solo.
- Leave `sidechain` out for none. A bypassed effect is keyed by nothing.
- Keys after the effects must not make a loop: a track that keys its own effect, or two that key each other, need `pre_fx` on one key. A loop, a track that does not exist and an effect with no `sidechain` input are reported in `problems.txt`, and the effect follows its own sound.

## Automation: lanes of a track

A track can move a number of one of its devices, or its own volume or pan, over time. Each number that moves has a lane in `automation` in the track record, with points in project ticks.

```json state/arrangement/riser/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Riser",
    "colour": "blue",
    "order": 3,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["dark"],
    "automation": [
      {
        "device": "dark",
        "parameter": "cutoff_hz",
        "points": [{"tick": 30720, "value": 300.0}, {"tick": 61440, "value": 8000.0}]
      },
      {"parameter": "gain_db", "points": [{"tick": 0, "value": "-inf"}, {"tick": 3840, "value": 0.0}]}
    ]
  }
}
```

```json state/arrangement/riser/dark.json
{
  "tool": "filter",
  "state": {
    "type": "low_pass",
    "cutoff_hz": 300.0,
    "resonance": 0.2,
    "slope": 12,
    "drive_db": 0.0,
    "mix": 1.0,
    "lfo_rate_hz": 1.0,
    "lfo_depth_octaves": 0.0
  }
}
```

In 4/4 the filter of this track opens over bars 9 to 16, and the track fades in over bar 1.

- `device`: the file name of a device in the track folder, without `.json`, such as an effect in `effects`. Leave it out for the volume and the pan of the track itself.
- `parameter`: the field in the record of the device, as its doc names it, such as `cutoff_hz` of a filter. A number inside an object or a list is named by its path: `filter_1.cutoff_hz` of a wavetable, `bands[0].gain_db` of an EQ, `pads.42.pan` of a Drum pad, `parameters.12.value` of a plugin. For the track itself, `gain_db` or `pan`.
- `points[].tick`: in project ticks, not from the start of a clip. In tick order, at most one per tick, and at least one per lane.
- `points[].value`: in the units and the range of the field, as its doc gives them. A volume may go down to `"-inf"`.
- Between two points the value moves in a straight line on the travel of its knob: a cutoff moves evenly in octaves, a volume as its fader moves. Before the first point and after the last it holds, as in a clip lane.
- While a lane moves a number, the value in the record does not play. Take the lane out and it plays again.
- A lane belongs to the track, not to a clip. When you move a clip, by its `start` or to another track, move the points under it yourself if they belong to it.
- One number has one lane. Leave `automation` out when the track has none.
- Every built-in instrument and effect takes automation, and so do the volume and pan of every track. A whole number, such as a count of voices, takes none. A plugin takes it for the parameters its record pins, see `agent-docs/plugins.md`.

## The arrangement and its master: `arrangement`

```json state/arrangement/instance.json
{
  "tool": "arrangement",
  "state": {
    "master": {
      "gain_db": 0.0,
      "limiter": {"bypass": false, "gain_db": 0.0, "ceiling_db": 0.0, "release_ms": 100.0, "lookahead_ms": 0.0}
    }
  }
}
```

The arrangement is the master: every track plays into it, and it plays to the main output. `{"state": {}}` is the same as the example.

- `master.gain_db`: the volume of the master, a number up to 6, or `"-inf"` for silence. It comes before the limiter, so it cannot push the output over the ceiling.
- `master.limiter`: on by default, so the output never goes over its ceiling. It has the fields of `agent-docs/limiter.md`, and `bypass`: `true` lets the sound through untouched, and it may then clip. Its defaults differ: `ceiling_db` 0, and `lookahead_ms` 0, which may be any number from 0 to 10. A lookahead above 0 also delays a keyboard played live.
