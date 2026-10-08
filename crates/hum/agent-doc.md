# Hum, the language of a tool's sound

Hum is the small language the sound of a tool of this project is written in, in its `sound` (see `agent-docs/extensions.md`). It runs once for every sample, on each channel apart, in each voice apart. It is compiled when it changes and cannot crash, hang or allocate: every memory it uses is declared on its own line or made by one function call.

```text
param rate = 4 [0.1, 20]
wave = 0.5 + 0.5 * sin(phasor(rate) * tau)
out = in * (1 - wave)
```

## Lines

- `name = expression` sets a name. A name is set once and read on the lines below it.
- `out = expression` is the sample that leaves. Code that never sets `out` passes `in` through.
- `history name` declares a value that feeds back: reading it gives what it was set to in the sample before, 0 at first. Set it once, on a line below. A line after the one that sets it still reads the value of the sample before.
- `param name = default [min, max]` is a number of the record, a knob. It glides over 20 ms. The tool writes these lines for its knobs and toggles.
- `param name[length] = default [min, max]` is a list of the record, such as the steps of a sequence. Read it with `name[index]` or `lookup(name, phase)`.
- `live name = default [min, max]` is a number the interface plays and nothing saves, such as an XY pad. It glides over 20 ms, and starts at its default.
- `trigger name` is 1 in the one sample after the interface fires it, else 0: a bang.
- `watch name = expression` sets `name` and shows its value to the interface, as the card reads it: a meter, a step light.
- `buffer name = seconds` is memory to write and read, such as a loop or a grain cloud, all 0 at first. Write with `name[index] = expression`, read with `name[index]` or `lookup(name, phase)`.
- `// ...` is a comment.

## What it reads

| Name | Is |
| --- | --- |
| `in` | The sample that comes in. An effect only; 0 in an instrument or a source. |
| `channel` | 0 left, 1 right. |
| `sr` | The sample rate. |
| `pi`, `tau` | 3.14159..., 6.28318... |
| `freq`, `pitch` | The note of the voice: in Hz, with the bend wheel, and as a MIDI number, 69 is A4. |
| `gate` | 1 while the note is held, 0 after. |
| `velocity` | How hard the note was played, 0 to 1. |
| `onset` | 1 in the first sample of a note, else 0. |
| `beat` | Quarter notes from the start of the piece, while it plays; it stands still while stopped. `wrap(beat / 4)` goes 0 to 1 once a bar in 4/4. |
| `bpm`, `playing` | The tempo, and 1 while the piece plays. |

Operators: `+ - * / %`, and `< > <= >= == !=`, which give 1 for true and 0 for false. `-` in front of a value negates it.

## Functions

| Function | Gives |
| --- | --- |
| `sin(x)`, `cos(x)`, `tan(x)`, `tanh(x)`, `abs(x)`, `sqrt(x)`, `exp(x)`, `log(x)`, `floor(x)`, `pow(x, power)`, `min(a, b)`, `max(a, b)` | The math functions. Angles are in radians. |
| `clamp(x, low, high)` | `x` held between `low` and `high`. |
| `wrap(x)` | The part of `x` after the point: 0 to 1. |
| `mix(a, b, amount)` | `a` at 0, `b` at 1, in between for the values in between. |
| `db(decibels)` | The gain of a level in dB: `db(-6)` is about 0.5. |
| `saturate(x)` | Clean up to full scale, then bends softly; never above 1.5. |
| `phasor(hz)` | A ramp from 0 to 1, `hz` times a second. `sin(phasor(hz) * tau)` is a sine. |
| `noise()` | White noise from -1 to 1, different on each channel. |
| `delay(x, ms)` | `x` as it was `ms` milliseconds ago, up to 4000. `ms` may move every sample and reads between samples smoothly, so a moving delay bends the pitch without clicks: a chorus, a tape wobble. `delay(x, ms, longest)` holds only `longest` ms, a number; give it in an instrument, where every voice keeps its own. |
| `lowpass(x, hz, q)`, `highpass(x, hz, q)`, `bandpass(x, hz, q)` | A filter. `q` may be left out: 0.707, no peak. Higher rings at `hz`, up to 20. |
| `smooth(x, ms)` | `x` that follows changes slowly, in about `ms` milliseconds. |
| `adsr(gate, attack, decay, sustain, release)` | An envelope from 0 to 1: up in `attack` ms while `gate` is above 0, down to `sustain` in `decay` ms, and to 0 in `release` ms after. |
| `rise(x)` | 1 in the sample where `x` goes from 0 or below to above 0, else 0: a clock from a phasor is `rise(wrap(beat * 4) < 0.5)`, sixteenths. |
| `change(x)` | 1 in the sample where `x` differs from the sample before. |
| `hold(x, when)` | `x` as it was the last time `when` was above 0: sample and hold. |
| `name[index]` | The value of a list or buffer at `index`: the whole part, wrapped, so any index reads. |
| `lookup(name, phase)` | The list or buffer read from `phase` 0 to 1 over its whole length, wrapped, with a straight line between values: a wavetable, a sample played at any speed. |
| `len(name)` | How many values a list or buffer holds; a buffer of 2 seconds holds `2 * sr`. |

Every call of `phasor`, `noise`, `delay`, a filter, `smooth`, `adsr`, `rise`, `change` and `hold` has a memory of its own. Call it once and use the name, for one oscillator heard in two places. The two channels have their own memories but start together, so a `phasor` moves the same on both.

## Effect, instrument, source

The tool says which it is (`kind` in `agent-docs/extensions.md`):

- An **effect** reads `in`, and one voice runs all the time.
- An **instrument** runs the code once per held note, each voice with its own memory, up to its `voices` at once. A voice starts at a note's `onset` with `gate` 1, and ends when `gate` is 0 and it has been silent for 50 ms; so multiply its sound by an `adsr` of `gate`, or it rings until it is silent by itself.
- A **source** is one voice that runs all the time, notes or not: a drone, a texture, a generative part. `freq`, `gate` and `velocity` follow the newest held key; `gate` is 0 before the first note.

## Safety and limits

A sample that leaves is held to 4, about 12 dB over full scale, and a value that is not a number is 0, so a feedback that runs away is loud, not dangerous; keep feedback gains under 1. At most 32 params, 32 live controls, 32 triggers, 16 watches, 16 delays and 30 s of buffers. A code error names its line and says what to write instead.
