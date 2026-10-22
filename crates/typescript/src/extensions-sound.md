# The sound of a tool

Read `agent-docs/extensions.md` first. Every function here has its doc in `extensions/sdk/sound.ts`.

- Methods take a signal or a number: `.plus`, `.minus`, `.times`, `.over`, `.mod`, `.negate()`; the comparisons `.lt`, `.gt`, `.le`, `.ge`, `.eq`, `.ne` give 1 for true and 0 for false.
- `channel` is 0 left, 1 right, 0 in a stereo sound. `note.freq` (Hz, with bend), `note.pitch`, `note.gate` (1 while held), `note.velocity` and `note.onset` are the note of a voice. `beat` counts quarter notes from the start while the piece plays.
- Oscillators go from -1 to 1. `phasor(hz)` is a ramp from 0 to 1 to build others.
- `delay(x, ms, longest?)` goes up to 4000 ms, smooth when `ms` moves (chorus, tape wobble); give `longest` when the tool has more voices than one.
- `rise(x)` is 1 where `x` goes above 0, a clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth. `hold(x, when)` samples and holds.
- A table is a list of numbers such as a `pattern`, a `file` field or a `buffer`: `.at(index)` reads its whole part, wrapped; `lookup(table, phase)` reads 0 to 1 over the table, smooth. A file plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`; grains read short stretches of it.

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

## Inside `sound` only

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains.
- `output(name, ...)`: sends out what a parent connects to another child (`agent-docs/extensions-children.md`). A signal is a number, once per 64 samples, which its view also reads as `now.value(name)`: a meter, a step light. `{ bang: when }` is a bang on the sample `when` goes above 0, such as an onset. `{ gate, pitch, velocity? }` are notes: one starts where `gate` goes above 0, at the MIDI `pitch` and `velocity` (0 to 1) of that sample, and ends where the gate falls or the pitch moves. Up to 16 numbers and 8 of the others; a sound of more voices than one sends only numbers, its newest note's.

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
