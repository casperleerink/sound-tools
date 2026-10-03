# Sampler

`sampler` plays one audio file across the keyboard, or a sampled instrument of many files written as an SFZ file: see "SFZ instruments" below. As the `instrument.json` of a track it plays the notes of that track, like the synth.

With one file, the root key plays the file at its own pitch, and a key an octave up plays it twice as fast and an octave higher.

The file must be in `assets/audio/` of the project, and the record names it by its file name. To use a file from elsewhere, copy it in first, under a name of lowercase letters, digits, `-` and `_`, such as `assets/audio/kalimba.wav`. WAV, AIFF and FLAC, any sample rate.

A new track that plays a sample: copy the file in, write the track, then its instrument, then clips of notes as `agent-docs/arrangement.md` says. Give the track an `order` above the highest one in use and a `colour` no other track has.

```json state/arrangement/kalimba/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Kalimba", "colour": "teal", "order": 7, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/kalimba/instrument.json
{
  "tool": "sampler",
  "state": {
    "sample": "kalimba.wav",
    "root": 60,
    "start_seconds": 0.0,
    "attack_seconds": 0.002,
    "decay_seconds": 0.4,
    "sustain": 1.0,
    "release_seconds": 0.3,
    "velocity_to_volume": 0.5,
    "gain_db": 0.0
  }
}
```

These are the defaults, with a sample. A field you leave out takes its default, so `{"sample": "kalimba.wav"}` is enough. Without `sample` the Sampler is silent.

| Field | Meaning | Values |
| --- | --- | --- |
| `sample` | The file under `assets/audio/` it plays. | a file name, such as `"kalimba.wav"` |
| `root` | The key that plays the file at its own pitch, as a note number: 60 is C4, 69 is A4. Set it to the pitch of the sound in the file. | 0 to 127 |
| `start_seconds` | Where in the file every note starts. Skip silence or a click at the start of a file with it. | 0 up to the length of the file |
| `end_seconds` | Where in the file a note stops at the latest. Leave it out to play to the end of the file. | after `start_seconds` |
| `attack_seconds` | From note on to full level. | 0.001 to 10 |
| `decay_seconds` | From full level down to the sustain level. | 0.001 to 10 |
| `sustain` | The level a held note settles at. 1 plays the file as it is while the key is held. 0 makes every note a pluck that ends after its decay. | 0 to 1 |
| `release_seconds` | From note off to silence. | 0.001 to 10 |
| `velocity_to_volume` | How much velocity changes the volume. 0 plays every note at full volume. 1 plays velocity 64 a quarter as loud as 127, 0.5 plays it at 63 %. | 0 to 1 |
| `gain_db` | Output gain in dB. Set the level of a track with its `gain_db`. | -48 to 24 |

A note plays from `start_seconds` to `end_seconds`, or until its release ends, whichever is first. The file is never looped. A key far above the root plays the file fast and short.

Starting points: a sustained sound (strings, a pad, a held voice) keeps `sustain` 1 and gets `release_seconds` 0.5 or more. A plucked or struck sound (kalimba, piano, a drum) plays well with `sustain` 1 too, since the file decays by itself. To shorten one, set `sustain` 0 with `decay_seconds` as long as the part you want.

The envelope, `velocity_to_volume` and `gain_db` can move over time with an automation lane of the track: see `agent-docs/arrangement.md`.

An edit applies while notes sound. `gain_db` glides there over 20 ms; the envelope applies at once, also to held notes; the other fields apply from the next note. A new `sample` fades out the notes of the old one. A file that is not there is listed in `problems.txt` and the Sampler is silent until the file arrives; the rest of the project plays.

To play notes, write a clip into the track folder as `agent-docs/arrangement.md` says. The Sampler has the ports `notes` (in) and `audio` (out, stereo).

## The library

For a real instrument (piano, strings, brass, woodwinds, guitar, bass, drums), the Sampler plays instruments of a free library, by an id: `{"library": "strings/violin-section"}`. Open `agent-docs/library.md` for the instruments, their ranges and how they are downloaded.

## SFZ instruments

An SFZ instrument is a folder of samples and a text file, `.sfz`, that says which sample plays for which key and how hard it is played. Free instruments come in this format, and you can write one. The folder goes under `assets/instruments/`, and the record names the SFZ file by its path there, with `sfz` instead of `sample`. A new track that plays one:

```json state/arrangement/cello/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Cello", "colour": "sapphire", "order": 8, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/cello/instrument.json
{
  "tool": "sampler",
  "state": {
    "sfz": "cello/cello-sustain.sfz",
    "root": 60,
    "start_seconds": 0.0,
    "attack_seconds": 0.002,
    "decay_seconds": 0.4,
    "sustain": 1.0,
    "release_seconds": 0.3,
    "velocity_to_volume": 0.5,
    "gain_db": -3.0
  }
}
```

`{"sfz": "cello/cello-sustain.sfz"}` is enough; the runtime writes the other fields with their defaults. The SFZ file sets the pitch, envelope and velocity response of each sample, so `root`, `start_seconds`, `end_seconds`, the envelope and `velocity_to_volume` of the record do nothing with `sfz`. `gain_db` still applies. A record has `sample` or `sfz`, not both.

The folders and files of an instrument may have any names. Its samples may be anywhere under `assets/`, such as recordings in `assets/audio/`, but not outside it. Samples are WAV, AIFF or FLAC. A missing SFZ file or missing samples are listed in `problems.txt`; the rest plays, and files that arrive later play without an edit of the record. Opcodes the Sampler does not know are ignored, so a third-party SFZ file plays, at times plainer than in its own player.

To make an instrument from recordings or other samples, open `agent-docs/sfz.md`: how to write an SFZ file, and every opcode the Sampler plays.

A note plays at most 8 regions at once, and the Sampler 32 notes.
