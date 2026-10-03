# The library

Free sampled instruments for the Sampler: piano, keys, strings, brass, woodwinds, guitar, bass, drums, percussion and synths. For a real instrument, use one of these before a synth. A record names one by its id with `library`, instead of `sample` or `sfz`:

```json state/arrangement/violins/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Violins", "colour": "rosewater", "order": 9, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/violins/instrument.json
{
  "tool": "sampler",
  "state": {
    "library": "strings/violin-section",
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

`{"library": "strings/violin-section"}` is enough. As with `sfz`, only `gain_db` of the record applies: the instrument has its own pitch, envelope and velocity response.

An instrument plays once it is downloaded on this machine, once for every project. Nothing downloads by itself: until the composer clicks **Download** on the Sampler card, or picks the instrument there, `problems.txt` says it is not downloaded. Tell the composer which instruments to download and how big they are. A failed download is listed in `problems.txt` too, and **Try again** on the card retries it.

Range is where the instrument has samples. Write the parts in the range of the real instrument, which is often narrower.

| Id | Instrument | Range | Download |
| --- | --- | --- | --- |
| `piano/kawai-upright` | Upright piano (Kawai) | A0 to C8 | 33 MB |
| `piano/old-upright` | Old upright piano | G0 to C8 | 34 MB |
| `keys/wurlitzer` | Wurlitzer electric piano | A1 to C7 | 2 MB |
| `keys/cp80` | Yamaha CP80 electric grand | A0 to C8 | 11 MB |
| `keys/fm-piano` | FM electric piano | D#1 to D7 | 25 MB |
| `keys/pipe-organ` | Pipe organ | C2 to C7 | 46 MB |
| `strings/violin-section` | Violin section, sustain | G3 to D6 | 46 MB |
| `strings/viola-section` | Viola section, sustain | C3 to D6 | 72 MB |
| `strings/cello-section` | Cello section, sustain | C2 to F5 | 73 MB |
| `strings/double-bass-section` | Double bass section, sustain | C1 to C4 | 48 MB |
| `strings/violin-section-pizzicato` | Violin section, pizzicato | G3 to D6 | 9 MB |
| `strings/solo-violin` | Solo violin, sustain | G3 to C7 | 74 MB |
| `strings/harp` | Harp | E1 to F7 | 35 MB |
| `brass/trumpet` | Trumpet, sustain | E3 to C6 | 40 MB |
| `brass/french-horn` | French horn, sustain | A1 to F5 | 51 MB |
| `brass/trombone` | Trombone, sustain | A#1 to F4 | 60 MB |
| `brass/tuba` | Tuba, sustain | F1 to D4 | 31 MB |
| `woodwinds/flute` | Flute, sustain | C4 to C7 | 25 MB |
| `woodwinds/oboe` | Oboe, sustain | A#3 to F6 | 24 MB |
| `woodwinds/clarinet` | Clarinet, sustain | D3 to F#6 | 59 MB |
| `woodwinds/bassoon` | Bassoon, sustain | A#1 to D#5 | 35 MB |
| `woodwinds/alto-sax` | Alto sax | G#2 to A5 | 29 MB |
| `woodwinds/tenor-sax` | Tenor sax | D#2 to E5 | 28 MB |
| `guitar/classical` | Classical guitar (nylon) | F1 to E6 | 5 MB |
| `guitar/electric` | Electric guitar, clean | A1 to C7 | 123 MB |
| `guitar/electric-light` | Electric guitar, clean (light) | B1 to D6 | 3 MB |
| `bass/electric-finger` | Electric bass, finger | D1 to A2 | 3 MB |
| `bass/electric` | Electric bass | A-1 to C6 | 110 MB |
| `bass/double-bass-pizzicato` | Double bass, pizzicato | C0 to C9 | 137 MB |
| `drums/acoustic-kit` | Acoustic drum kit | drum map below | 139 MB |
| `drums/orchestral-percussion` | Orchestral percussion kit (GM layout) | drum map below | 155 MB |
| `drums/synth-kit` | Vintage synth drum kit | drum map below | 2 MB |
| `percussion/timpani` | Timpani | C2 to C4 | 36 MB |
| `percussion/glockenspiel` | Glockenspiel | G4 to C7 | 6 MB |
| `percussion/marimba` | Marimba | F2 to C7 | 12 MB |
| `percussion/world` | World percussion | C3 to B4 | 8 MB |
| `synth/bass` | Synth bass | C1 to E6 | 1 MB |
| `synth/sweep-pad` | Sweep pad | A1 to C7 | 3 MB |

## Drum maps

The kits play one sound per key.

`drums/acoustic-kit`: 48 kick, 49 kick (other beater), 50 snare, 51 snare (second), 52 closed hi-hat, 53 open hi-hat, 54 ride, 55 ride bell, 56 ride (second), 57 ride bell (second), 58 crash, 59 crash (second), 60 china, 61 to 64 toms from high to low, 65 and 66 snare with the snares off.

`drums/synth-kit`: 48 and 49 kicks, 50 and 51 snares, 52 and 53 closed hi-hats, 54 open hi-hat, 55 and 56 cymbals, 57 and 58 low toms, 59 and 60 mid toms, 61 and 62 high toms, 63 clap, 64 shaker, 65 long shaker, 66 claves.

`drums/orchestral-percussion`: 32 to 35 bass drum rubs, 36 bass drum, 37 snare taps, 38 snare hit, 39 snare roll, 40 and 41 hit and roll with the snares off, 42 gong scrape, 46 gong, 47, 48 and 50 cymbal swells (short, medium, long), 49 crash cymbals, 51 suspended cymbal, 53 tambourine shake, 54 tambourine hit, 55 tambourine roll, 56 cowbell, 59 cymbal stick hit, 60 to 65 quinto, conga and tumba (tap, hit), 67 anvil, 68 brake drum, 69 to 71 ratchet, 72 guiro, 75 claves, 76 and 77 log drums, 78 to 81 triangles, 82 sleigh bells, 83 to 86 bell tree, 88 effect, 94 vibraslap, 101 to 104 bowed cymbal.
