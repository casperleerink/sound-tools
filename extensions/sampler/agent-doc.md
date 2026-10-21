# Sampler

`sampler` plays one audio file across the keyboard, or a sampled instrument of many files written as an SFZ file: see "SFZ instruments" below. It plays the notes that reach its input `notes`, like the synth.

With one file, the root key plays the file at its own pitch, and a key an octave up plays it twice as fast and an octave higher.

The file must be in `assets/audio/` of the project, and the record names it by its file name. To use a file from elsewhere, copy it in first, under a name of lowercase letters, digits, `-` and `_`, such as `assets/audio/kalimba.wav`. WAV, AIFF and FLAC, any sample rate.

To play a sample: copy the file in, then write the Sampler where a parent plays notes into it:

```json state/song/kalimba/synth.json
{
  "tool": "sampler",
  "state": {"sample": "kalimba.wav", "root": 72}
}
```

The kalimba in this file sounds a C5, so `root` is 72. `{"sample": "kalimba.wav"}` alone plays it with every other field at its default. Without `sample` the Sampler is silent.

{{fields sampler}}

A note plays from `start_seconds` to `end_seconds`, or until its release ends, whichever is first. With `reverse` it plays from `end_seconds` (or the end of the file) back to `start_seconds`. A key far above the root plays the file fast and short.

With `loop` a note plays from `start_seconds` to the end once, then repeats the part from `loop_start_seconds` to the end until its release has faded it out, so a short file can hold a long note. The loop point gets a short crossfade, so it does not click. With `reverse` the loop plays the same part backwards: from the end back to `loop_start_seconds`, then from the end again.

Starting points: a sustained sound (strings, a pad, a held voice) keeps `sustain` 1 and gets `release_seconds` 0.5 or more. A plucked or struck sound (kalimba, piano, a drum) plays well with `sustain` 1 too, since the file decays by itself. To shorten one, set `sustain` 0 with `decay_seconds` as long as the part you want.

The envelope, `velocity_to_volume` and `gain_db` can move over time with a `lane` child of its parent (`agent-docs/lane.md`).

An edit applies while notes sound. `gain_db` glides there over 20 ms; the envelope applies at once, also to held notes; the other fields apply from the next note. A new `sample` fades out the notes of the old one. A file that is not there is listed in `problems.txt` and the Sampler is silent until the file arrives; the rest of the project plays.

The Sampler has the ports `notes` (in) and `audio` (out, stereo); a `timeline` child of its parent plays notes into it (`agent-docs/timeline.md`).

## The library

For a real instrument (piano, strings, brass, woodwinds, guitar, bass, drums), the Sampler plays instruments of a free library, by an id: `{"library": "strings/violin-section"}`. Open `agent-docs/library.md` for the instruments, their ranges and how they are downloaded.

## SFZ instruments

An SFZ instrument is a folder of samples and a text file, `.sfz`, that says which sample plays for which key and how hard it is played. Free instruments come in this format, and you can write one. The folder goes under `assets/instruments/`, and the record names the SFZ file by its path there, with `sfz` instead of `sample`:

```json state/song/cello/synth.json
{
  "tool": "sampler",
  "state": {"sfz": "cello/cello-sustain.sfz", "gain_db": -3.0}
}
```

`{"sfz": "cello/cello-sustain.sfz"}` is enough; the runtime writes the other fields with their defaults. The SFZ file sets the pitch, envelope and velocity response of each sample, so `root`, `tune`, `start_seconds`, `end_seconds`, `reverse`, `loop`, `loop_start_seconds`, the envelope and `velocity_to_volume` of the record do nothing with `sfz`. `gain_db` still applies. A record has `sample` or `sfz`, not both.

The folders and files of an instrument may have any names. Its samples may be anywhere under `assets/`, such as recordings in `assets/audio/`, but not outside it. Samples are WAV, AIFF or FLAC. A missing SFZ file or missing samples are listed in `problems.txt`; the rest plays, and files that arrive later play without an edit of the record. Opcodes the Sampler does not know are ignored, so a third-party SFZ file plays, at times plainer than in its own player.

To make an instrument from recordings or other samples, open `agent-docs/sfz.md`: how to write an SFZ file, and every opcode the Sampler plays.

A note plays at most 8 regions at once, and the Sampler 32 notes.
