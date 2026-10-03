# SFZ: making an instrument

An SFZ file is a text file that maps samples to keys and velocities. The Sampler plays one when its record names it with `sfz`, as `agent-docs/sampler.md` says. This doc is how to write one.

To turn a few recorded notes into an instrument, copy them into a folder under `assets/instruments/`, find the pitch of each (from its name, or ask the composer), and write one `<region>` per sample. Each covers the keys up to halfway to its neighbours.

```text
assets/instruments/my-guitar/
  my-guitar.sfz
  samples/e2.wav  samples/a2.wav  samples/d3.wav
```

```sfz
// My guitar, three notes.
<global> ampeg_release=0.4

<region> sample=samples/e2.wav lokey=36 hikey=42 pitch_keycenter=40
<region> sample=samples/a2.wav lokey=43 hikey=47 pitch_keycenter=45
<region> sample=samples/d3.wav lokey=48 hikey=60 pitch_keycenter=50
```

## The format

A header starts a part: `<region>` is one sample, `<group>` holds opcodes for the regions after it, and `<global>` holds them for all. A region's own opcode wins over its group's, and the group's over the global one. `//` starts a comment.

| Opcode | Meaning |
| --- | --- |
| `sample` | The file, relative to the SFZ file. |
| `lokey`, `hikey`, `key` | The keys it plays, as numbers (60) or names (`c4`, `f#3`). `key` sets both and `pitch_keycenter`. |
| `pitch_keycenter` | The key at which the sample plays at its own pitch. |
| `lovel`, `hivel` | The velocities it plays, 1 to 127: one layer per dynamic. |
| `seq_length`, `seq_position` | Round robin: regions of one key that take turns, so repeated notes do not sound the same. |
| `lorand`, `hirand` | Plays when a random value per note, 0 to 1, falls in this range. |
| `sw_lokey`, `sw_hikey`, `sw_last`, `sw_default` | Keyswitches: a key in the `sw_` range picks the articulation and plays nothing, and a region plays only under its `sw_last`. |
| `trigger=release` | Plays when the key comes up, such as a damper or a fret noise. |
| `offset`, `end` | The first and the last frame of the file that play. |
| `loop_mode`, `loop_start`, `loop_end` | `no_loop`, `one_shot` (plays to its end, as a drum), `loop_continuous`, `loop_sustain` (loops while held). With none given, the loop saved in a WAV file plays. |
| `volume`, `amplitude`, `pan` | Level in dB, level in percent, pan from -100 to 100. |
| `tune`, `transpose`, `pitch_keytrack` | Cents, semitones, and cents per key: 0 for a drum that plays one pitch on every key. |
| `amp_veltrack` | How much velocity changes the level, in percent, 100 by default. |
| `ampeg_attack`, `ampeg_decay`, `ampeg_sustain`, `ampeg_release` | The envelope in seconds, and the sustain in percent. |
| `group`, `off_by`, `off_mode` | A note of group `n` stops the regions with `off_by=n`: a closed hi-hat cuts an open one. |
| `default_path` | In `<control>`: a folder put before every `sample`. |
| `#define $NAME value`, `#include "file.sfz"` | A name for a value, and the text of another file put in its place. |

A note plays at most 8 regions at once, and the Sampler 32 notes. Anything else in an SFZ file is ignored.
