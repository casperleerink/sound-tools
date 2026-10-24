# The sound of a tool

Read `agent-docs/extensions.md` first. Every function here has its doc in `extensions/sdk/sound.ts`.

- Methods take a signal or a number: `.plus`, `.minus`, `.times`, `.over`, `.mod`, `.negate()`; the comparisons `.lt`, `.gt`, `.le`, `.ge`, `.eq`, `.ne` give 1 for true and 0 for false.
- `channel` is 0 left, 1 right, 0 in a stereo sound. `note.freq` (Hz, with bend), `note.pitch`, `note.gate` (1 while held), `note.velocity` and `note.onset` are the note of a voice. `beat` counts quarter notes from the start while the piece plays.
- Oscillators go from -1 to 1; `phasor(hz)`, a ramp from 0 to 1, builds others.
- Filters: `lowpass`, `highpass`, `bandpass`, `notch`, `allpass` take `(x, hz, q?)`; `peak`, `lowshelf`, `highshelf` take `(x, hz, db, q?)`.
- `delay(x, ms, longest?)` goes up to 4000 ms, smooth when `ms` moves (chorus, tape wobble); give `longest` when the tool has more voices than one.
- `rise(x)` is 1 where `x` goes above 0, a clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth. `hold(x, when)` samples and holds.
- A table is a list of numbers such as a `pattern`, a `file` field or a `buffer`: `.at(index)` reads its whole part, wrapped; `lookup(table, phase)` reads 0 to 1 over the table, smooth. A file plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`; grains read short stretches of it.

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

Up to 3 more sound inputs and outputs: `inputs: { sidechain: audio() }` is read as `sidechain`, like `input`, and `sidechain.left`, `.right`; the DAW's sidechain picker keys any effect with a `sidechain`. With `outputs: { low: audio() }`, `sound` returns `{ audio, low }`, each a signal or each `{ left, right }`.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

## Inside `sound` only

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains.
- `output(name, ...)`: sends out what a parent connects to another child (`agent-docs/extensions-children.md`). A signal is a number, once per 64 samples, which its view also reads as `now.value(name)`: a meter, a step light. `{ bang: when }` is a bang on the sample `when` goes above 0, such as an onset. `{ gate, pitch, velocity? }` are notes: one starts where `gate` goes above 0, at the MIDI `pitch` and `velocity` (0 to 1) of that sample, and ends where the gate falls or the pitch moves. Up to 16 numbers and 8 of the others; a sound of more voices than one sends only numbers, its newest note's.

## The spectrum

`spectral(x, size, (bin) => ({ mag, phase }))` takes `x` apart into bins of frequency and runs the function once per bin, every `size / 4` samples; what it returns is the bin that goes out. `bin.mag` is how loud the bin is (1 for a sine of 1), `bin.phase` where its wave is, in radians, and `bin.hz` and `bin.index` which bin it is. `size` is a power of two from 256 to 8192: 2048 for most uses, smaller to follow quick changes, larger to tell close notes apart.

- Each bin has its own memory, so `feedback`, `hold` and `smooth` work per bin. A `feedback` gives what it was set to in the spectrum before.
- A signal from outside, such as a field, is read once per spectrum. No `delay`, `buffer` or `output` inside.
- The sound comes out `size` samples late. The project makes up for it, but not inside the sound: mix in the dry sound in the bins, where `bin.mag` and `bin.phase` as they come give `x` back.
- In a tool of one voice only.
- `spectral([x, other], size, (bin, other) => ...)` also gets the bin of `other`, such as a `sidechain`, at the same frequency; what goes out is still the spectrum of `x`.

A freeze: the bins hold their level, and each wave turns on by its last step.

```ts
import { boolean, feedback, hold, input, mix, spectral, tool } from "./sdk";

tool({
  name: "freeze",
  title: "Freeze",
  when: "You want to hold a sound and let it ring on",
  doc: "While `frozen` is on, the sound of that moment rings on; off, the sound passes.",
  state: { frozen: boolean({ default: false }) },
  sound: ({ frozen }) =>
    spectral(input, 2048, (bin) => {
      const live = frozen.lt(0.5);
      const last = feedback();
      last.set(bin.phase);
      const step = hold(bin.phase.minus(last), live);
      const turned = feedback();
      const phase = mix(turned.plus(step), bin.phase, live);
      turned.set(phase);
      return { mag: hold(bin.mag, live), phase };
    }),
});
```

## Limits

A sound is held to 4, about 12 dB over full scale, and a value that is not a number is 0, so a feedback that runs away is loud, not dangerous. Keep feedback gains under 1. With more voices than one, a voice ends when its gate is 0 and it has been silent for 50 ms with nothing left in its `delay`s, so multiply it by an `adsr` of `note.gate`.

```ts
import { delay, feedback, input, lowpass, mix, number, saturate, tool } from "./sdk";

tool({
  name: "dark-echo",
  title: "Dark echo",
  when: "You want echoes that get darker as they repeat",
  doc: "An echo whose repeats lose their highs. `time` is between repeats, `again` how much comes back.",
  state: {
    time: number({ min: 10, max: 2000, default: 350, unit: "ms" }),
    again: number({ min: 0, max: 0.95, default: 0.5 }),
  },
  sound: ({ time, again }) => {
    const echo = feedback();
    const wet = delay(input.plus(echo.times(again)), time);
    echo.set(saturate(lowpass(wet, 2500)));
    return mix(input, wet, 0.4);
  },
});
```
