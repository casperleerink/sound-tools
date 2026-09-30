# Wavetable

`wavetable` is a synth that plays wavetables: tables of single-cycle waves, called frames, that it morphs between. As the `instrument.json` of a track it plays the notes of that track. Each note has two wavetable oscillators, a sine sub, two filters, an amp envelope, two more envelopes, two LFOs, and a modulation matrix that ties them together.

A new track with the Wavetable: write the track, then its instrument, then clips of notes as `agent-docs/arrangement.md` says. Give the track an `order` above the highest one in use and a `colour` no other track has.

```json state/arrangement/synth/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "Synth", "colour": "sapphire", "order": 8, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/synth/instrument.json
{
  "tool": "wavetable",
  "state": {
    "osc_1": {
      "on": true,
      "table": "basic_shapes",
      "position": 0.5,
      "effect": "none",
      "effect_amount": 0.4,
      "octave": 0,
      "semitone": 0,
      "detune_cents": 0.0,
      "gain": 0.7,
      "pan": 0.0
    },
    "osc_2": {
      "on": true,
      "table": "basic_shapes",
      "position": 0.5,
      "effect": "none",
      "effect_amount": 0.4,
      "octave": 0,
      "semitone": 0,
      "detune_cents": 7.0,
      "gain": 0.7,
      "pan": 0.0
    },
    "sub": {"octave": -1, "gain": 0.0},
    "unison": {"voices": 1, "amount": 0.3},
    "filter_1": {
      "on": true,
      "type": "low_pass",
      "slope": 24,
      "cutoff_hz": 1000.0,
      "resonance": 0.2,
      "drive_db": 0.0
    },
    "filter_2": {
      "on": false,
      "type": "low_pass",
      "slope": 24,
      "cutoff_hz": 1000.0,
      "resonance": 0.2,
      "drive_db": 0.0
    },
    "routing": "serial",
    "amp_env": {
      "attack_seconds": 0.005,
      "decay_seconds": 0.4,
      "sustain": 0.6,
      "release_seconds": 0.3,
      "attack_curve": 0.5,
      "decay_curve": 0.8,
      "release_curve": 0.8
    },
    "env_2": {
      "attack_seconds": 0.005,
      "decay_seconds": 0.4,
      "sustain": 0.6,
      "release_seconds": 0.3,
      "attack_curve": 0.5,
      "decay_curve": 0.8,
      "release_curve": 0.8
    },
    "env_3": {
      "attack_seconds": 0.005,
      "decay_seconds": 0.4,
      "sustain": 0.6,
      "release_seconds": 0.3,
      "attack_curve": 0.5,
      "decay_curve": 0.8,
      "release_curve": 0.8
    },
    "lfo_1": {
      "shape": "sine",
      "rate_hz": 1.0,
      "sync": false,
      "division": "1/4",
      "feel": "straight",
      "retrigger": true
    },
    "lfo_2": {
      "shape": "sine",
      "rate_hz": 1.0,
      "sync": false,
      "division": "1/4",
      "feel": "straight",
      "retrigger": true
    },
    "voicing": {"mode": "poly", "polyphony": 8, "glide_seconds": 0.0},
    "matrix": [
      {"source": "env_2", "destination": "filter_1_cutoff", "amount": 0.4},
      {"source": "key", "destination": "filter_1_cutoff", "amount": 0.5},
      {"source": "velocity", "destination": "amp_level", "amount": 0.5},
      {"source": "mod_wheel", "destination": "osc_1_position", "amount": 0.5}
    ],
    "gain": 0.15
  }
}
```

This is the default patch: two saws, the second 7 cents up, through a low pass that the second envelope opens at the start of each note. Higher keys open it more, softer notes are quieter, and the mod wheel morphs the first saw into a square. `"state": {}` is this patch, so write only what you change. Inside an object, a field you leave out takes the default of the tables below, which differs from the patch above only in `osc_2.detune_cents`, 0, and `filter_2.on`, true. A `matrix` you write replaces the whole matrix.

An edit applies while notes sound, without a click: numbers and choices glide over 20 ms, and a new table or effect fades its oscillator out and in over 10 ms each way.

## Oscillators

`osc_1` and `osc_2` read a table at a position and play it at the pitch of the note. Between two frames they play a mix of both, so moving the position morphs the sound. `on` is `true` or `false`.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `table` | The table, one of the built-in tables below. | a table name | `"basic_shapes"` |
| `position` | Where in the table it reads: 0 is its first frame, 1 its last. | 0 to 1 | 0.5 |
| `effect` | How it bends the table as it reads it: `"none"`; `"fm"`, a sine at its pitch bends where it reads, best on smooth tables; `"sync"`, reads each cycle up to 16 times faster, as hard sync does; `"warp"`, squeezes the start of each cycle and stretches its end; `"fold"`, drives it into a wavefolder. | one of those | `"none"` |
| `effect_amount` | How far the effect bends it. 0 is the table as it is. | 0 to 1 | 0.4 |
| `octave` | Octaves up or down from the note. | -3 to 3 | 0 |
| `semitone` | Semitones up or down. | -12 to 12 | 0 |
| `detune_cents` | Hundredths of a semitone up or down. A few cents between the two oscillators makes a wider sound. | -50 to 50 | 0 |
| `gain` | Its level into the filters. | 0 to 1 | 0.7 |
| `pan` | -1 is left, 1 is right. | -1 to 1 | 0 |

The tables, each made in code, with its category:

| Table | Category | From position 0 to 1 |
| --- | --- | --- |
| `basic_shapes` | basic | sine, triangle, saw at 0.5, square at 1 |
| `pulse_width` | basic | a square that narrows to a thin pulse |
| `harmonics` | additive | a sine, then the harmonics of a saw added one by one up to 32 |
| `organ` | additive | drawbar settings of a tonewheel organ, from one flute to all drawbars out |
| `vowels` | vocal | the vowels a, e, i, o, u, one into the next |
| `resonant_sweep` | spectral | a saw through a resonant low pass that opens |
| `sync_sweep` | synthesis | a saw hard-synced up to three octaves higher |
| `fm_sweep` | synthesis | a sine whose phase another sine bends more and more |
| `fold_sweep` | synthesis | a sine driven harder and harder into a wavefolder |
| `digital` | digital | a sine at fewer and fewer levels, down to three steps |

## Sub

`sub` is a sine one or two octaves under the note, in the middle, into the filters. `octave` is `-1` or `-2`.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `gain` | Its level. 0 is off. | 0 to 1 | 0 |

## Unison

`unison` plays copies of both oscillators per note, spread in pitch and from left to right. More copies are about as loud as one, and cost more.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `voices` | Copies per note. 1 is no unison. | 1 to 8 | 1 |
| `amount` | How far they spread: at 1, 50 cents up and down and from left to right. | 0 to 1 | 0.3 |

## Filters

`filter_1` and `filter_2` are the filter of the Filter effect, per note. `on` is `true` or `false`; off lets the sound through. `type` is `"low_pass"`, `"band_pass"`, `"high_pass"` or `"notch"`, and `slope` is `12` or `24` dB per octave. `routing` says where the sound goes: `"serial"`, everything through filter 1 and then filter 2; `"parallel"`, everything through both side by side, half of each; `"split"`, oscillator 1 through filter 1, oscillator 2 through filter 2, and the sub half through each.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `cutoff_hz` | Where it starts to cut, or the middle of its band or gap. | 20 to 20000 | 1000 |
| `resonance` | 0 is no peak, 1 a strong ringing peak at the cutoff. | 0 to 1 | 0.2 |
| `drive_db` | Gain into a soft saturation before the filter. 0 is clean. | 0 to 24 | 0 |

## Envelopes

`amp_env` shapes the level of each note. `env_2` and `env_3` are sources for the matrix, from 0 to 1. Each goes up to 1 in the attack, down to the sustain level in the decay, and to 0 in the release after the key is let go.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `attack_seconds` | From note on to the top. | 0.001 to 10 | 0.005 |
| `decay_seconds` | From the top to the sustain level. | 0.001 to 10 | 0.4 |
| `sustain` | Where a held note settles. 0 makes every note a pluck. | 0 to 1 | 0.6 |
| `release_seconds` | From note off to silence. | 0.001 to 10 | 0.3 |
| `attack_curve` | How the attack bends: 0 is a straight line, 1 a strong curve, fast at first and slow near its end. | 0 to 1 | 0.5 |
| `decay_curve` | The same for the decay. | 0 to 1 | 0.8 |
| `release_curve` | The same for the release. | 0 to 1 | 0.8 |

## LFOs

`lfo_1` and `lfo_2` are sources for the matrix, from -1 to 1. `shape` is `"sine"`, `"triangle"`, `"saw_up"`, `"saw_down"`, `"square"` or `"sample_and_hold"`, a new random level each cycle, the same on every render. With `sync` `true` a cycle is a note of the tempo, `division` (`"1/32"`, `"1/16"`, `"1/8"`, `"1/4"`, `"1/2"` or `"1/1"`) and `feel` (`"straight"`, `"dotted"` or `"triplet"`), and `rate_hz` is not used. With `retrigger` `true` each note starts its own LFO from the start of its cycle; `false`, notes pick up one LFO that runs on, and while the project plays a synced one is in time with the bars.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `rate_hz` | Cycles per second. | 0.01 to 40 | 1 |

## Voicing

`voicing` has `mode`, `"poly"` or `"mono"`. Mono plays one note: a key pressed while another is held moves the note there without starting it again, and letting go goes back to the key still held. Bend moves every note two semitones either way, and the mod wheel adds a vibrato, as on every built-in instrument.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `polyphony` | Notes that play at once in poly. | 1 to 16 | 8 |
| `glide_seconds` | How long a note slides from the pitch of the last one. 0 is no glide. | 0 to 5 | 0 |

## Matrix

`matrix` is a list of up to 16 routes. Each moves a destination by `amount` times its source; routes to the same destination add up.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `amount` | How much, and which way. | -1 to 1 | 0 |

Sources, with their range: `env_2` and `env_3`, 0 to 1; `lfo_1` and `lfo_2`, -1 to 1; `velocity`, how hard the note was played, 0 to 1; `key`, the pitch of the note, 0 at middle C (60) and 1 eight octaves up; `mod_wheel`, 0 to 1; `pressure`, how hard the keys are pressed, 0 to 1; `random`, a value from -1 to 1 each note draws when it starts, the same on every render.

What amount 1 does with the source at 1:

| Destination | Amount 1 adds |
| --- | --- |
| `osc_1_position`, `osc_2_position` | the whole table, 1 |
| `osc_1_effect`, `osc_2_effect` | the whole effect amount, 1 |
| `osc_1_pitch`, `osc_2_pitch` | 24 semitones |
| `osc_1_gain`, `osc_2_gain`, `sub_gain` | the whole gain, 1 |
| `filter_1_cutoff`, `filter_2_cutoff` | 8 octaves: `key` at amount 1 makes the cutoff follow the keys exactly |
| `filter_1_resonance`, `filter_2_resonance` | the whole resonance, 1 |
| `pan` | from the middle to one side |
| `lfo_1_rate`, `lfo_2_rate` | 4 octaves, 16 times faster |
| `unison_amount` | the whole unison amount, 1 |

`amp_level` works differently: a route to it only turns a note down, by nothing where the source is at its top and by the amount where it is at its bottom. `velocity` at 1 makes each note as loud as it was played; an LFO at 1 is a full tremolo; a negative amount turns the note down where the source is high instead.

## Output

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `gain` | Linear output gain. One note of the default patch at full velocity peaks at about this value. Tracks add up into the master, whose limiter squashes a loud sum, so keep it near 0.15, lower for thick chords and unison. Set the level of a track with its `gain_db`. | 0 to 1 | 0.15 |

Starting points: a pad is `amp_env.attack_seconds` 0.5 or more, `release_seconds` 1 or more, and an LFO on `osc_1_position`. A pluck is `amp_env.sustain` 0 with `decay_seconds` 0.2 to 0.5. A bass is `octave` -1 with `sub.gain` 0.5 and `cutoff_hz` 300 to 600. A lead is `voicing.mode` `"mono"` with `glide_seconds` 0.05 to 0.2. Morphing sounds come from routing an envelope or an LFO to a position.
