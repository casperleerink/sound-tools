# The sound of a tool

Read `agent-docs/extensions.md` first. Every function here has its doc in `extensions/sdk/sound.ts`.

- The comparisons `.lt`, `.gt`, `.le`, `.ge`, `.eq`, `.ne` give 1 for true and 0 for false.
- `channel` is 0 left, 1 right, 0 in a stereo sound. `note.freq` (Hz, with bend), `note.pitch`, `note.gate` (1 while held), `note.velocity` and `note.onset` are the note of a voice. `beat` counts quarter notes from the start while the piece plays.
- Oscillators go from -1 to 1; `phasor(hz)`, a ramp from 0 to 1, builds others.
- Filters: `lowpass`, `highpass`, `bandpass`, `notch`, `allpass` take `(x, hz, q?)`; `peak`, `lowshelf`, `highshelf` take `(x, hz, db, q?)`.
- `rise(x)` is 1 where `x` goes above 0, a clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth. `hold(x, when)` samples and holds.
- A table is a list of numbers such as a `pattern`, a `file` field or a `buffer`: `.at(index)` reads its whole part, wrapped; `lookup(table, phase)` reads 0 to 1 over the table, smooth. A file plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`, in stereo: a sound that returns `{ left, right }` hears it mixed, or one side as `sound.left` and `sound.right`.

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

Up to 3 more sound inputs and outputs: `inputs: { sidechain: audio() }` is read as `sidechain`, like `input`, and `sidechain.left`, `.right`; the DAW's sidechain picker keys any effect with a `sidechain`. With `outputs: { low: audio() }`, `sound` returns `{ audio, low }`, each a signal or each `{ left, right }`.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

## Inside `sound` only

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains. The buffers and delays of a tool hold up to 16 minutes together, and 2 minutes in a tool of more voices than one.
- `output(name, ...)`: sends out what a parent connects to another child (`agent-docs/extensions-children.md`). A signal is a number, once per 64 samples, which its view also reads as `now.value(name)`: a meter, a step light. `{ bang: when }` is a bang on the sample `when` goes above 0, such as an onset. `{ gate, pitch, velocity? }` are notes: one starts where `gate` goes above 0, at the MIDI `pitch` and `velocity` (0 to 1) of that sample, and ends where the gate falls or the pitch moves. Up to 16 numbers, 8 bangs and 8 notes; a sound of more voices than one sends only numbers, its newest note's.

## The spectrum

`spectral(x, size, (bin) => ({ mag, phase }))` runs the function once per bin of frequency; what it returns is the bin that goes out. `bin.mag` is how loud the bin is (1 for a sine of 1), `bin.phase` where its wave is, in radians. A `size` of 2048 suits most uses, smaller follows quick changes, larger tells close notes apart.

- Each bin has its own memory: a `feedback` gives what it was set to in the spectrum before. No `delay`, `buffer` or `output` inside.
- The sound comes out about `size` samples late. The project makes up for it, but not inside the sound: mix in the dry sound in the bins, where `bin.mag` and `bin.phase` as they come give `x` back.

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

A sound is held to 4, about 12 dB over full scale, and a value that is not a number is 0, so a feedback that runs away is loud, not dangerous. Keep feedback gains under 1. With more voices than one, a voice ends only once its gate is 0 and it is silent, so multiply it by an `adsr` of `note.gate`.

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
