# Writing a tool for this project

When no tool does what the composer asks, write one: an effect, an instrument or a sound source that is part of this project, in TypeScript. It plays as soon as you save the file, with no build, and it is a tool like any other: its records go in a track, the window gives it a card and offers it under "This project", and the runtime writes a doc for it so the next agent can use it.

To play a track from the computer keyboard, write nothing: tell the composer to turn on the computer keys (cmd-K, or the keyboard button in the transport). The letter rows then play the selected track as a MIDI keyboard does, with any instrument, and record into a take.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own card. Make the folder if it is not there, then run `sound-tools . --inspect` once: the runtime writes `extensions/sdk.ts`, the SDK with every type and function, and `extensions/tsconfig.json`. Do not edit them; read `sdk.ts` when you need the exact types. A tool needs no entry in `project.json`.

The runtime runs them with Bun, which the app ships and puts on your `PATH`; a build from source uses the one installed from https://bun.sh. While the app has the project open, a save loads every file of `extensions/` again within a second, every record of the tool plays the new sound, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

## A tool

```ts
import { input, knob, phasor, sin, TAU, tool } from "./sdk";

tool({
  name: "wobble",
  title: "Wobble",
  when: "You want the volume of a sound to pulse",
  doc: "A tremolo: the volume moves up and down `rate` times a second. `depth` is how far it dips, 0 not at all, 1 to silence.",
  state: {
    rate: knob({ min: 0.1, max: 20, default: 4, unit: "hz" }),
    depth: knob({ min: 0, max: 1, default: 0.5 }),
  },
  sound: ({ rate, depth }) => {
    const wave = sin(phasor(rate).times(TAU)).times(0.5).plus(0.5);
    return input.times(depth.times(wave).negate().plus(1));
  },
});
```

- `name`: the `tool` of its records and the name of its doc. Lowercase letters, digits, `-` and `_`, and not the name of a built-in tool.
- `title`: what the card and the pickers say.
- `when` and `doc`: what the next agent reads in the map and in `agent-docs/<name>.md`. Say what the tool does to the sound and what each field does musically, also between its ends: is the middle of a knob half as much, or less? The runtime adds where the record goes and a table of the fields, so do not repeat those.
- `kind`: `"effect"` (the default) goes in a track's `effects` and hears the track through `input`. `"instrument"` is what a track plays, its `instrument.json`: the sound runs once per note, up to 8 at a time. `"source"` is what a track plays too, but one voice that runs all the time, notes or not: a drone, a texture, a generative part; it follows the newest held note.
- `state`: the fields of its record. A field name is lowercase letters, digits and `_`.
- `controls`: what its card plays and nothing saves, see below.
- `sound`: the sound, as a `Signal`.

## The sound

`sound` gets the fields and controls, and returns a `Signal`: a stream of samples, one value per sample, run for each channel apart and for each voice apart. It is called once, when the tool loads or a choice changes, to build the sound; the sound then plays by itself. So a knob is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

Combine signals with methods, which take a signal or a number: `.plus`, `.minus`, `.times`, `.over`, `.mod`, `.negate()`, and the comparisons `.lt`, `.gt`, `.le`, `.ge`, `.eq`, `.ne`, which give 1 for true and 0 for false. And with these functions, all in `sdk.ts` with their docs:

| | |
| --- | --- |
| What comes in | `input` (an effect), `channel` (0 left, 1 right; 0 in a stereo sound), `sampleRate`; `inputLeft`, `inputRight` for a stereo sound |
| The note of a voice | `note.freq` (Hz, with bend), `note.pitch` (MIDI, 69 is A4), `note.gate` (1 while held), `note.velocity` (0 to 1), `note.onset` (1 in its first sample) |
| The piece | `beat` (quarter notes from the start, while playing), `bpm`, `playing` |
| Math | `sin`, `cos`, `tan`, `tanh`, `abs`, `sqrt`, `exp`, `log`, `floor`, `wrap` (the part after the point), `min`, `max`, `pow`, `clamp`, `mix(a, b, amount)`, `db(decibels)`, `saturate`, `PI`, `TAU` |
| Oscillators | `sine(hz)`, `saw(hz)`, `square(hz, width?)`, `triangle(hz)`, from -1 to 1. `phasor(hz)`: a ramp 0 to 1 to build others. `noise()`: white noise. `mtof(pitch)`: the Hz of a MIDI pitch |
| Time | `delay(x, ms, longest?)` up to 4000 ms, smooth when `ms` moves (chorus, tape wobble); give `longest` in an instrument. `smooth(x, ms)`. `adsr(gate, attack, decay, sustain, release)` in ms |
| Filters | `lowpass`, `highpass`, `bandpass(x, hz, q?)` |
| Events | `rise(x)`: 1 where `x` goes above 0, a clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth. `change(x)`, `hold(x, when)`: sample and hold |
| Tables | a `pattern` field, a `sample` field and a `buffer`: `.at(index)` (whole part, wrapped), `lookup(table, phase)` (0 to 1 over the table, smooth), `.length`. A sample plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`; grains read short stretches of it |

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`: a ping-pong delay, a panner, a widener.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

Inside `sound` only:

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains.
- `watch(name, signal)`: shows the signal's value to the card, as `watches[name]`: a meter, a step light. Up to 16.

A field, a control or a watch cannot take a name the sound has built in: `in`, `in_left`, `in_right`, `channel`, `sr`, `pi`, `tau`, `beat`, `bpm`, `playing`, `freq`, `pitch`, `gate`, `velocity`, `onset`, `out`, `out_left`, `out_right`, `param`, `live`, `trigger`, `watch`, `history`, `buffer`, `sample`.

A sound is held to 4, about 12 dB over full scale, and a value that is not a number is 0, so a feedback that runs away is loud, not dangerous. Keep feedback gains under 1. An instrument's voice ends when its gate is 0 and it has been silent for 50 ms, so multiply it by an `adsr` of `note.gate`.

```ts
import { delay, feedback, input, knob, lowpass, mix, saturate, tool } from "./sdk";

tool({
  name: "dark-echo",
  title: "Dark echo",
  when: "You want echoes that get darker as they repeat",
  doc: "An echo whose repeats lose their highs. `time` is between repeats, `again` how much comes back.",
  state: {
    time: knob({ min: 10, max: 2000, default: 350, unit: "ms" }),
    again: knob({ min: 0, max: 0.95, default: 0.5 }),
  },
  sound: ({ time, again }) => {
    const echo = feedback();
    const wet = delay(input.plus(echo.times(again)), time);
    echo.set(saturate(lowpass(wet, 2500)));
    return mix(input, wet, 0.4);
  },
});
```

TypeScript builds what a hand would repeat: a bank of combs is a `map`.

```ts
const combs = [29.7, 37.1, 41.1, 43.7].map((ms) => {
  const comb = feedback();
  comb.set(lowpass(delay(input.plus(comb.times(size)), ms), 5000));
  return comb;
});
const wet = combs.reduce((sum, comb) => sum.plus(comb)).over(combs.length);
```

## Fields

| Field | Record holds | In `sound` | A change |
| --- | --- | --- | --- |
| `knob({ min, max, default, unit?, label? })` | a number | a `Signal` | glides at once |
| `toggle({ default, label? })` | `true` or `false` | a `Signal`, 1 or 0 | glides at once |
| `choice({ options, default, label? })` | one of `options` | the option itself | builds the sound again; it fades in over 10 ms |
| `pattern({ length, min, max, default, label? })` | a list of `length` numbers | a table: `.at(index)` | at once |
| `sample({ label? })` | the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound: one channel at the engine's rate, up to 60 s | at once, once the file is there |

`unit` is `"hz"`, `"ms"`, `"db"` or `"percent"`: the card shows it, a knob in `"hz"` turns on a log scale, and one in `"percent"` still holds 0 to 1. At most 32 knobs and toggles.

## Controls

What the card plays as a performer plays, which nothing saves and no undo takes back:

- `live({ min, max, default, label? })`: a number the card moves, such as an XY pad. A `Signal` in `sound`, gliding over 20 ms; it starts at its default.
- `trigger({ label? })`: a bang: a `Signal` that is 1 for the one sample after the card fires it.

## Its card

Without `card`, the card has a knob per knob and live control, a row of steps per pattern, and a button per option, toggle and trigger. For your own, give `card` in a `.tsx` file. It draws from the record and the watches and plays what it needs; it keeps no state of its own.

A card sits in the row of cards under the tracks. What it draws is 144 points tall at most: more is cut off. It is as wide as it needs, at least 200, and the row scrolls sideways. So put things side by side rather than stacked: a `Knob` is 56 by 72, so two rows of knobs fill a card; a `Pad` 120 square unless `size` says; a `Meter` 64 tall; `Steps` 14 tall; a line of text about 16. A `page` has the whole window.

```tsx
import { Knob, Meter, Pad, Steps, h, type Style, tool } from "./sdk";

const column: Style = { gap: 8 };
const row: Style = { direction: "row", gap: 8, align: "center" };
const button: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };

tool({
  // name, title, when, doc, state, controls and sound
  card: ({ state, watches, update, set, fire }) => (
    <div style={column}>
      <div style={row}>
        <Knob path="cutoff" label="Cutoff" min={100} max={8000} default={1200} unit="hz" />
        <Pad x="bend" y="drive" size={72} />
        <Meter watch="level" />
      </div>
      <div style={row}>
        <Steps path="steps" playing="step" />
        <div style={button} onClick={() => fire("hit")}>Hit</div>
      </div>
    </div>
  ),
});
```

| Element | Does |
| --- | --- |
| `<div style onClick>` | A box; `Style` in `sdk.ts` lists every style. Text goes inside. |
| `<Knob path label min max default unit?>` | Turns the field `path` of the record: a drag is one undo step. With `live="name"` instead of `path` it plays a live control. |
| `<Steps path max? playing?>` | A row of steps on a pattern: a click turns a step on (to `max`, the pattern's max by default) or off (to its min). `playing` names a watch whose value is the step that lights up. |
| `<SampleChooser path label?>` | The file of a sample field, and a button that opens one and copies it into `assets/audio/`. |
| `<Meter watch label?>` | A bar that shows a watch from 0 to 1. |
| `<Pad x y size?>` | A square for the pointer: across moves the live control `x`, up moves `y`. |

The card gets `state`, the record; `watches`, the last value of each watch, and draws again as they move; `update(label, change)`, which changes the record as one undo step; `set(control, value)`, which moves a live control; `fire(control)`, which fires a trigger; and `play(pitch, { seconds?, velocity? })`, which plays a note on the tool's own voices, as a key held for `seconds` (an instrument or a source).

## Experiments: a control loop, a canvas, a page

For more than knobs, such as a simulation that plays notes as balls bounce, a tool keeps a `memory` and runs a control loop:

- `memory: () => ({ ... })` makes what the tool keeps and nothing saves, per instance. The card and the loop get it, and a click may change it.
- `tick: ({ state, watches, memory, dt, time, set, fire, play }) => { ... }` runs about 30 times a second while the window is open, `dt` seconds apart. It plays the sound as a performer would, with `set`, `fire` and `play`, and moves what is in `memory`; it does not change the record. Offline, in `--render`, it does not run. With `play`, an instrument is a voice per note the loop makes up: a melody of a rule, a chord a collision strikes.
- What the loop plays at once lands up to 30 ms late. For a steady rhythm, play ahead on the clock of the sound: `time` is where it is, in seconds, and `fire(control, { at })` and `play(pitch, { at })` happen on the sample of `at`:

  ```ts
  tick: ({ memory, time, play }) => {
    if (memory.next < time) memory.next = time; // after a pause, start from now
    while (memory.next < time + 0.1) {
      play(60, { at: memory.next });
      memory.next += 0.25;
    }
  },
  ```

  `at` is for the loop: a card has no `time`.
- `play(pitch, { hold: true })` holds a note until `release(pitch)`.
- `onKey: (tool, { key, down }) => { ... }` hears the computer keyboard: each key going down and up again, by name (`"a"`, `"1"`, `"space"`, `"left"`). It hears after a click on the tool's page or card, and a page that opens with the window hears at once; plain keys are then the tool's, so space does not play. `onMidi: (tool, message) => { ... }` hears the MIDI keyboard while the tool is on the track it plays or at the top of the project: `{ type: "noteOn", pitch, velocity }`, `{ type: "noteOff", pitch }`, `{ type: "cc", controller, value }` (the pedal is 64) and `{ type: "bend", value }`, values 0 to 1, the bend -1 to 1. An instrument plays the notes by itself as well. Both get what `tick` gets, and `update`, which changes the record as a card does:

  ```ts
  onKey: ({ play, release }, { key, down }) => {
    const step = "awsedftgyhuj".indexOf(key); // a row of the keyboard as piano keys
    if (step < 0) return;
    if (down) play(60 + step, { hold: true });
    else release(60 + step);
  },
  ```
- `<Canvas width height shapes background? onPress? onDrag?>` draws `shapes` (`{ kind: "circle", x, y, radius, color }`, `{ kind: "rect", x, y, width, height, color, radius? }`, `{ kind: "line", from: [x, y], to: [x, y], color, width? }`, in points from its top left) and hears the pointer: `onPress(x, y)` and `onDrag(x, y)` from 0 to 1 across and down. The card draws again after every tick.
- `page: (page) => ...` draws the whole window instead of a card, from what a card gets and `size`, its width and height in points. Fill it: the window may be any size, and the page draws again when it changes. Keep positions from 0 to 1 and scale them by `size` as it draws. A project that is one experiment has no arrangement: delete `state/arrangement/`, put one record of the tool at the top, `state/<name>.json`, make the tool a `source` (or an `instrument` when its loop plays notes with `play`), and connect it to the speakers in `project.json` (see `agent-docs/project-json.md`): `{"from": {"instance": "<name>", "port": "audio"}, "to": {"device_output": 0}}`. The window shows its page. An `effect` at the top with `{"from": {"device_input": 0}, "to": {"input": {"instance": "<name>", "port": "audio"}}}` as well hears the microphone live as `input`; tell the composer to use headphones, as speakers feed back into it. `{"from": {"app": "Music"}, ...}` in place of `device_input` hears another app instead, such as a song playing in Music, and `{"app": "all"}` every app.

```tsx
import { Canvas, feedback, h, live, lowpass, max, noise, tool, trigger, type Shape } from "./sdk";

type Ball = { x: number; y: number; speed: number };

tool({
  name: "rain",
  title: "Rain",
  when: "You want rain drops you can drop with the pointer",
  doc: "Each drop that lands clicks; a press drops one where it is.",
  kind: "source",
  state: {},
  controls: { drop: trigger(), bright: live({ min: 200, max: 8000, default: 2000 }) },
  memory: () => ({ balls: [] as Ball[] }),
  sound: ({ drop, bright }) => {
    // A trigger is 1 for one sample: a level that jumps to it and falls away is its envelope.
    const level = feedback();
    level.set(max(drop, level.times(0.995)));
    return lowpass(noise(), bright).times(level);
  },
  tick: ({ memory, dt, fire }) => {
    for (const ball of memory.balls) {
      ball.speed += 2 * dt;
      ball.y += ball.speed * dt;
      if (ball.y > 1) {
        ball.y = 1;
        ball.speed *= -0.5;
        fire("drop");
      }
    }
  },
  page: ({ memory, size }) => (
    <Canvas
      width={size.width}
      height={size.height}
      shapes={memory.balls.map((ball): Shape => ({ kind: "circle", x: ball.x * size.width, y: ball.y * size.height, radius: 5, color: "#60a5fa" }))}
      onPress={(x, y) => memory.balls.push({ x, y, speed: 0 })}
    />
  ),
});
```

## Check your work

1. `bun x tsc -p extensions` checks the types. The first run downloads TypeScript.
2. `problems.txt`, or the problems `sound-tools . --inspect` prints, list what is wrong under `extensions/<file>`: a file that does not load, a tool definition that does not hold, a sound that does not build, and, while the window is open, the first tick or click that failed. Fix it and save; the problem goes.
3. Use the tool as the composer will: write its record in a track (`"state": {}` is its defaults; an effect is also named in the track's `effects`). `--inspect` lists each track's instrument and effects with their tools. Then measure it with `agent-docs/inspect.md`: `--analyze` shows the loudness and the bands of each moment.

## Limits

A tool is sound in or out, with notes for an instrument or a source. Its control loop, `onKey` and `onMidi` run only while the window is open. Its knobs can be automated by the lanes of a track, as the numbers of any device. For a sound a built-in tool already makes, use the built-in one.
