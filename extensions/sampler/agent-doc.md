# Sampler

`sampler` plays one audio file across the keyboard, an SFZ instrument, or a library instrument, from the notes its parent plays into it (`agent-docs/timeline.md`).

Copy the file into `assets/audio/` first, under a name of lowercase letters, digits, `-` and `_`. WAV, AIFF and FLAC. Then write the Sampler:

```json state/song/kalimba/synth.json
{
  "tool": "sampler",
  "state": {"sample": "kalimba.wav", "root": 72}
}
```

The kalimba in this file sounds a C5, so `root` is 72.

{{fields sampler}}

Without `loop`, a note ends at the end of the file even while the key is held. With `loop` it plays to the end once, then repeats from `loop_start_seconds`, so a short file can hold a long note.

Starting points: a sustained sound (strings, a pad, a held voice) keeps `sustain` 1 and gets `release_seconds` 0.5 or more. A plucked or struck sound plays well with `sustain` 1 too. To shorten one, set `sustain` 0 with `decay_seconds` as long as the part you want.

A `lane` (`agent-docs/lane.md`) can move the envelope, `velocity_to_volume` and `gain_db`. The ports are `notes` (in) and `audio` (out, stereo).

## The library

For a real instrument (piano, strings, brass, woodwinds, guitar, bass, drums), use the free library by id, `{"library": "strings/violin-section"}`: see `agent-docs/library.md`.

## SFZ instruments

An SFZ instrument is a folder of samples and an `.sfz` text file that maps them to keys and velocities. The folder goes under `assets/instruments/`; `sfz` names the SFZ file by its path there:

```json state/song/cello/synth.json
{
  "tool": "sampler",
  "state": {"sfz": "cello/cello-sustain.sfz", "gain_db": -3.0}
}
```

With `sfz`, only `gain_db` of the record applies. A record has `sample` or `sfz`, not both. The samples may be anywhere under `assets/`, not outside it. Opcodes the Sampler does not know are ignored.

To write an SFZ file, open `agent-docs/sfz.md`.
