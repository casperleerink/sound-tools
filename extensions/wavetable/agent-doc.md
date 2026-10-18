# Wavetable

`wavetable` is a synth that plays wavetables: tables of single-cycle waves, called frames, that it morphs between. As the `instrument.json` of a track it plays the notes of that track. Each note has two wavetable oscillators, a sine sub, two filters, an amp envelope, two more envelopes, two LFOs, and a modulation matrix that ties them together.

Here a pad on a new track:

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

`"state": {}` is the default patch: two saws, the second 7 cents up, through a low pass that `env_2` opens at the start of each note, with `filter_2` off. Its `matrix` is the first four routes above: higher keys open the filter more, softer notes are quieter, and the mod wheel morphs the first saw into a square. Write only what you change. Inside an object you write, a field you leave out takes the default of the tables below. A `matrix` you write replaces the whole matrix, so this pad keeps the four routes and adds an LFO that slowly morphs the first saw.

Every number here but `octave`, `semitone`, `voices`, `polyphony` and the matrix can move over time with an automation lane of the track, named by its path: `filter_1.cutoff_hz`. See `agent-docs/arrangement.md`.

An edit applies while notes sound, without a click: numbers and choices glide over 20 ms, and a new table or effect fades its oscillator out and in over 10 ms each way.

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

`filter_1` and `filter_2` are the filter of the Filter effect, per note. `routing` above says where the sound goes through them.

{{fields wavetable filter_1}}

## Envelopes

`amp_env`, `env_2` and `env_3`. Each goes up to 1 in the attack, down to the sustain level in the decay, and to 0 in the release after the key is let go.

{{fields wavetable amp_env}}

## LFOs

`lfo_1` and `lfo_2`:

{{fields wavetable lfo_1}}

## Voicing

Bend moves every note two semitones either way, and the mod wheel adds a vibrato, as on every built-in instrument.

{{fields wavetable voicing}}

## Matrix

Each route of `matrix` moves a destination by `amount` times its source; routes to the same destination add up.

{{fields wavetable matrix}}

Starting points: a pad is `amp_env.attack_seconds` 0.5 or more, `release_seconds` 1 or more, and an LFO on `osc_1_position`. A pluck is `amp_env.sustain` 0 with `decay_seconds` 0.2 to 0.5. A bass is `octave` -1 with `sub.gain` 0.5 and `cutoff_hz` 300 to 600. A lead is `voicing.mode` `"mono"` with `glide_seconds` 0.05 to 0.2. Morphing sounds come from routing an envelope or an LFO to a position.
