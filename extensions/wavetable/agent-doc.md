# Wavetable

`wavetable` is a synth that morphs through tables of single-cycle waves. It plays the notes that reach its input `notes`.

Here a pad:

```json state/song/synth/synth.json
{
  "tool": "wavetable",
  "state": {
    "amp_env": {"attack_seconds": 0.8, "release_seconds": 1.5},
    "lfo_1": {"rate_hz": 0.2},
    "matrix": [
      {"source": "env_2", "destination": "filter_1_cutoff", "amount": 0.4},
      {"source": "key", "destination": "filter_1_cutoff", "amount": 0.5},
      {"source": "velocity", "destination": "amp_level", "amount": 0.5},
      {"source": "mod_wheel", "destination": "osc_1_position", "amount": 0.5},
      {"source": "lfo_1", "destination": "osc_1_position", "amount": 0.3}
    ]
  }
}
```

`"state": {}` is the default patch: two saws through a low pass that `env_2` opens, with the first four routes above as its `matrix`. Write only what you change; a field you leave out inside an object takes its default. A `matrix` you write replaces the whole matrix, so this pad repeats the four default routes and adds one.

Every number but `octave`, `semitone`, `voices`, `polyphony` and the matrix can move with a lane, named by its path: `filter_1.cutoff_hz`.

## The record

{{fields wavetable}}

## Oscillators

`osc_1` and `osc_2`:

{{fields wavetable osc_1}}

## Sub

{{fields wavetable sub}}

## Unison

{{fields wavetable unison}}

## Filters

`filter_1` and `filter_2`:

{{fields wavetable filter_1}}

## Envelopes

`amp_env`, `env_2` and `env_3`:

{{fields wavetable amp_env}}

## LFOs

`lfo_1` and `lfo_2`:

{{fields wavetable lfo_1}}

## Voicing

{{fields wavetable voicing}}

## Matrix

{{fields wavetable matrix}}

Starting points: a pad is `amp_env.attack_seconds` 0.5 or more, `release_seconds` 1 or more, and an LFO on `osc_1_position`. A pluck: `amp_env.sustain` 0, `decay_seconds` 0.2 to 0.5. A bass: `octave` -1, `sub.gain` 0.5, `cutoff_hz` 300 to 600. A lead: `voicing.mode` `"mono"`, `glide_seconds` 0.05 to 0.2. A morphing sound routes an envelope or an LFO to a position.
