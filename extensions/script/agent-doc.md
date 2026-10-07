# Script: an effect you write as a small script

`script` is an effect whose sound is a few lines of code in its record. Use it when no built-in effect does what the composer asks: a tremolo with an odd shape, a tape delay that darkens each echo, a bit of noise under a sound. It goes in a track's `effects` like any effect. When you write the record, the new code plays at once, faded in over 10 ms, with no build.

Here the pad plays through a tremolo named `tremolo`:

```json state/arrangement/pad/tremolo.json
{
  "tool": "script",
  "state": {
    "code": [
      "param rate = 4 [0.1, 20]",
      "param depth = 0.5 [0, 1]",
      "",
      "// 0 to 1, once per cycle",
      "wave = 0.5 + 0.5 * sin(phasor(rate) * tau)",
      "out = in * (1 - depth * wave)"
    ],
    "values": {"rate": 6.0}
  }
}
```

| Field | Meaning |
| --- | --- |
| `code` | The script, one line per string. |
| `values` | Where each param stands, by name. A param left out is at the default of its line. Leave it out to use every default. |

## The language

The script runs once for every sample, on each channel apart. It reads the sample that comes in as `in` and sets `out` to the sample that leaves. A script that never sets `out` passes the sound through.

- `name = expression` sets a name. A name is set once and read on the lines below it.
- `param name = default [min, max]` is a number the composer turns: a knob, and a field of `values`. A change of it glides over 20 ms, so it does not click.
- `history name` declares a value that feeds back: reading it gives what it was set to in the sample before, 0 at first. Set it once, on a line below.
- `// ...` is a comment.

Built in: `in`, `channel` (0 left, 1 right), `sr` (the sample rate), `pi`, `tau`.

Operators: `+ - * / %`, and `< > <= >= == !=`, which give 1 for true and 0 for false. `-` in front of a value negates it.

| Function | Gives |
| --- | --- |
| `sin(x)`, `cos(x)`, `tan(x)`, `tanh(x)`, `abs(x)`, `sqrt(x)`, `exp(x)`, `log(x)`, `floor(x)`, `pow(x, power)`, `min(a, b)`, `max(a, b)` | The math functions. Angles are in radians. |
| `clamp(x, low, high)` | `x` held between `low` and `high`. |
| `wrap(x)` | The part of `x` after the point: 0 to 1. |
| `mix(a, b, amount)` | `a` at 0, `b` at 1, in between for the values in between. |
| `db(decibels)` | The gain of a level in dB: `db(-6)` is about 0.5. |
| `saturate(x)` | Clean up to full scale, then bends softly; never above 1.5. |
| `phasor(hz)` | A ramp from 0 to 1, `hz` times a second. `sin(phasor(hz) * tau)` is a sine. |
| `noise()` | White noise from -1 to 1. |
| `delay(x, ms)` | `x` as it was `ms` milliseconds ago, up to 4000. |
| `lowpass(x, hz, q)`, `highpass(x, hz, q)`, `bandpass(x, hz, q)` | A filter. `q` may be left out: 0.707, no peak. Higher rings at `hz`, up to 20. |
| `smooth(x, ms)` | `x` that follows changes slowly, in about `ms` milliseconds. |

Every call of `phasor`, `noise`, `delay`, a filter or `smooth` has a memory of its own. Call it once and use the name, for one oscillator heard in two places.

A sound that leaves the script is held to 4, about 12 dB over full scale, and a value that is not a number is 0. A feedback that runs away is loud, not dangerous; keep feedback gains under 1.

## Examples

A tape echo whose repeats get darker and softer:

```json state/arrangement/vocal/tape-echo.json
{
  "tool": "script",
  "state": {
    "code": [
      "param time = 350 [1, 2000]",
      "param feedback = 0.45 [0, 0.95]",
      "param tone = 2500 [200, 12000]",
      "param blend = 0.35 [0, 1]",
      "history echo",
      "wet = delay(in + echo * feedback, time)",
      "echo = saturate(lowpass(wet, tone))",
      "out = mix(in, wet, blend)"
    ]
  }
}
```

A bit crusher:

```json state/arrangement/drums/crush.json
{
  "tool": "script",
  "state": {
    "code": ["param bits = 6 [1, 16]", "steps = pow(2, bits)", "out = floor(in * steps + 0.5) / steps"]
  }
}
```

## When the code is wrong

A record whose code does not compile does not load: the old code keeps playing, and `problems.txt` says what is wrong as `code[<index>]`, the index of the line in `code`, from 0. Fix that line.
