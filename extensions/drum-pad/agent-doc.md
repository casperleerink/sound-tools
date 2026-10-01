# Drum pad: drums on 16 pads

`drum-pad` is an instrument with 16 pads. Each note from 36 to 51 plays one pad, on the General MIDI drum map, so a drum part written for any other drum machine plays the right sounds. Every pad is a drum sound made by synthesis, or a sample: an audio file under `assets/audio/`. A new Drum pad is the kit below and needs no files.

A drum part is a track whose `instrument.json` is a Drum pad, and note clips on it, as in `agent-docs/arrangement.md`. Here a track named `beat`:

```json state/arrangement/beat/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Beat", "colour": "peach", "order": 3, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/beat/instrument.json
{
  "tool": "drum-pad",
  "state": {"pads": {}}
}
```

`"pads": {}` is the kit as it is. That is all a drum part needs: write its notes. The kit belongs to the version of the app: a later version may change how its pads sound, and a pad you wrote out keeps what you wrote.

## Notes: the pads

| Note | Pad | Sound | Volume | Pitch | Decay | Pan | Choke |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 36 | Kick | `kick` | 0 | 0 | 600 | 0 | no |
| 37 | Rim | `rim` | 0 | 0 | 70 | 0 | no |
| 38 | Snare | `snare` | 0 | 0 | 350 | 0 | no |
| 39 | Clap | `clap` | 0 | 0 | 380 | 0 | no |
| 40 | Snare 2 | `snare` | -1 | 3 | 240 | 0 | no |
| 41 | Tom 1 | `tom` | 0 | -7 | 800 | 0.35 | no |
| 42 | Hat | `hat` | 0 | 0 | 180 | 0 | yes |
| 43 | Tom 2 | `tom` | 0 | -4 | 720 | 0.25 | no |
| 44 | Pedal hat | `hat` | -3 | -1 | 110 | 0 | yes |
| 45 | Tom 3 | `tom` | 0 | -2 | 650 | 0.1 | no |
| 46 | Open hat | `open_hat` | 0 | 0 | 650 | 0 | yes |
| 47 | Tom 4 | `tom` | 0 | 1 | 600 | -0.05 | no |
| 48 | Tom 5 | `tom` | 0 | 3 | 550 | -0.15 | no |
| 49 | Crash | `crash` | 0 | 0 | 1800 | -0.25 | no |
| 50 | Tom 6 | `tom` | 0 | 6 | 500 | -0.25 | no |
| 51 | Ride | `ride` | 0 | 0 | 2500 | 0.25 | no |

A note plays its pad once, as a hit: its `length` does not matter, write a sixteenth (240 ticks). `velocity` is how hard: 64 is a quarter as loud as 127, so ghost notes are 30 to 50 and accents 110 to 127. Another note of the same pad cuts the pad's last hit short, and so does a hit of a pad of the choke group for every other pad of it: a closed hat stops the open hat. Notes outside 36 to 51 play nothing.

A beat is a note clip. A quarter note is 960 ticks, so an eighth is 480 and a sixteenth 240. This clip is two beats: the kick on 1 and the "and" of 2, the snare on 2, the hat on every eighth and the open hat on the last one:

```json state/arrangement/beat/groove.json
{
  "tool": "arrangement.clip",
  "state": {
    "start": 0,
    "length": 1920,
    "notes": [
      {"start": 0, "length": 240, "pitch": 36, "velocity": 120},
      {"start": 0, "length": 240, "pitch": 42, "velocity": 100},
      {"start": 480, "length": 240, "pitch": 42, "velocity": 70},
      {"start": 960, "length": 240, "pitch": 38, "velocity": 112},
      {"start": 960, "length": 240, "pitch": 42, "velocity": 100},
      {"start": 1440, "length": 240, "pitch": 36, "velocity": 100},
      {"start": 1440, "length": 240, "pitch": 46, "velocity": 90}
    ]
  }
}
```

For a longer part write one clip of several bars, or one clip per bar or per pattern. Start from a steady kick, snare and hat, then add ghost notes, open hats and fills on the toms (41 low to 50 high) into a crash (49) on the next downbeat.

## Changing the pads

Write only the pads you change, by note. A pad you leave out is the pad of the kit, and a field you leave out of a pad is that pad's value in the kit. The runtime writes every pad that differs from the kit with all its fields. Here the open hat rings longer and pad 48 plays a sample:

```json state/arrangement/beat/instrument.json
{
  "tool": "drum-pad",
  "state": {
    "pads": {
      "46": {
        "sound": "open_hat",
        "volume_db": 0.0,
        "pitch_semitones": 0.0,
        "decay_ms": 900.0,
        "pan": 0.0,
        "choke": true
      },
      "48": {
        "sample": "shaker.wav",
        "volume_db": -3.0,
        "pitch_semitones": 0.0,
        "decay_ms": 1500.0,
        "pan": 0.0,
        "choke": false
      }
    }
  }
}
```

| Field | Meaning | Values |
| --- | --- | --- |
| `sound` | A sound of the kit: `"kick"`, `"snare"`, `"clap"`, `"rim"`, `"hat"`, `"open_hat"`, `"tom"`, `"crash"`, `"ride"`. | Write `sound` or `sample`, not both |
| `sample` | An audio file in `assets/audio/`, by its file name. Copy the file in first, see `agent-docs/audio.md`. | WAV or AIFF |
| `volume_db` | The level of the pad. | -48 to 12 |
| `pitch_semitones` | Up or down. A sound of the kit is tuned and keeps its length; a sample plays faster and higher, or slower and lower, as on a sampler. | -24 to 24 |
| `decay_ms` | How long the pad sounds, from the hit to silence. A sound of the kit is made for this length; a sample fades out over it. | 10 to 10000 |
| `pan` | -1 is left, 1 is right. | -1 to 1 |
| `choke` | In the choke group: a hit of it cuts every other pad of the group. | `true` or `false` |

Set a sample pad's `decay_ms` a little longer than its file, else its end is faded out; the window sets it to one and a half times the file. The last 2 ms of a sample always fade to silence, and a sample plays at most 10 s. A pad whose file is not there is silent and listed in `problems.txt`, and the other pads play; it plays once the file is in `assets/audio/`.

The volume and the pan of a pad can move over time with an automation lane of the track, named by the note of the pad: `pads.42.volume_db` or `pads.42.pan`. See `agent-docs/arrangement.md`.

Starting points: a deeper, longer kick is `pitch_semitones` -2 to -4 with `decay_ms` 800 to 1200. A tight snare is `decay_ms` 150 to 200. Toms are one sound, tuned apart: keep them in steps of 2 to 3 semitones. Hats too loud in a busy part: bring the hat pads to -4 to -6 dB rather than lowering their velocities. An edit applies while the part plays. Volume and pan glide over 20 ms; sound, pitch and decay apply from the next hit once the new sound is made, a moment after the edit.
