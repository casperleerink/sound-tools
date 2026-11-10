# The sound of a tool

Read `agent-docs/extensions.md` first. Every function here has its doc in `extensions/sdk/sound.ts`.

- A clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth while the piece plays.
- A `file` field plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`.

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

Up to 3 more sound inputs and outputs: `inputs: { sidechain: audio() }` is read as `sidechain`, like `input`, and `sidechain.left`, `.right`; the DAW's sidechain picker keys any effect with a `sidechain`. With `outputs: { low: audio() }`, `sound` returns `{ audio, low }`, each a signal or each `{ left, right }`.

## Inside `sound` only

`feedback()`, `buffer(seconds)`, `output(name, ...)` and `spectral` work only while `sound` runs. A number `output` is sent once per block of up to 64 samples. A sound sends up to 16 numbers, 8 bangs and 8 notes; one of more voices than one sends only numbers, its newest note's.

## The spectrum

`spectral(x, size, (bin) => ({ mag, phase }))` runs the function once per bin of frequency; what it returns is the bin that goes out. A `size` of 2048 suits most uses, smaller follows quick changes, larger tells close notes apart.

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
