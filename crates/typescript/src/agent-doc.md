# Writing a tool for this project

When no tool does what the composer asks, write one in TypeScript. It plays as soon as you save the file, with no build, and the runtime writes a doc for it so the next agent can use it. A tool takes one of two shapes:

- **A device in a track**: an effect, an instrument or a sound source, used in the arrangement as a built-in one is. Its records go in tracks, it shows a card in the row under the tracks.
- **The project is the tool**: an experiment, a game, a thing to play live. There is no arrangement: one record at the top, and a page that fills the window. See "A project that is one tool".

Both are the same `tool(...)`: the sound, the fields, the control loop and children work in both.

To play a track from the computer keyboard, write nothing: with the computer keys on (cmd-K, or the keyboard button in the transport), the letter rows play the selected track as a MIDI keyboard does.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own card or page. Make the folder if it is not there, then run `sound-tools . --inspect` once: the runtime writes `extensions/sdk.ts`, the SDK with every type and function, and `extensions/tsconfig.json`. Do not edit them; read `sdk.ts` when you need the exact types. A tool needs no entry in `project.json`.

The runtime runs them with Bun, which the app ships and puts on your `PATH`. While the app has the project open, a save of code or JSON in `extensions/` starts the tools again in a moment: every file and helper loads again, every `memory` starts over, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

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
- `sound`: the sound, as a `Signal`. Left out, it passes `input` through.

## The sound

`sound` gets the fields and controls, and returns a `Signal`: a stream of samples, one value per sample, run for each channel apart and for each voice apart. It is called when the tool loads, once for each combination of choices (at most 256), to build the sound; the sound then plays by itself. So a knob is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

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

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

Inside `sound` only:

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains.
- `watch(name, signal)`: shows the signal's value to the card, as `watches[name]`: a meter, a step light. Up to 16.
- `output(name, ...)`: sends out what a parent connects to another child (see "Children"): a signal as a number, once per 64 samples; `{ bang: when }`, a bang on the sample `when` goes above 0, such as an onset; `{ gate, pitch, velocity? }`, notes: one starts where `gate` goes above 0, at the MIDI `pitch` and the `velocity` (0 to 1) of that sample, and ends where the gate falls or the pitch moves. Up to 8 of each; an instrument sends only numbers, its newest note's.

A sound is held to 4, about 12 dB over full scale, and a value that is not a number is 0, so a feedback that runs away is loud, not dangerous. Keep feedback gains under 1. An instrument's voice ends when its gate is 0 and it has been silent for 50 ms with nothing left in its `delay`s, so multiply it by an `adsr` of `note.gate`.

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
const combs: Signal[] = [29.7, 37.1, 41.1, 43.7].map((ms) => {
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
| `choice({ options, default, label? })` | one of `options` | the option itself | switches to the sound of that option; it fades in over 10 ms |
| `pattern({ length, min, max, default, label? })` | a list of `length` numbers | a table: `.at(index)` | at once |
| `sample({ label? })` | the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound: one channel at the engine's rate, up to 60 s | at once, once the file is there |
| `notes({ default?, label? })` | a list of notes, `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`, times in ticks (960 a quarter) | nothing: data | its behaviour runs again |
| `text({ default?, label? })` | text | nothing: data | its behaviour runs again |
| `number({ min, max, integer?, default?, label? })` | a number; `default` is `min` when left out | nothing: data | its behaviour runs again |
| `list(item, { max?, default?, label? })` | a list of `item`: a field, or an object of fields such as `{ name: text(), notes: notes() }` | nothing: data | its behaviour runs again |

`unit` is `"hz"`, `"ms"`, `"db"` or `"percent"`: the card shows it, a knob in `"hz"` turns on a log scale, and one in `"percent"` still holds 0 to 1. At most 32 knobs and toggles.

## Controls

What the card plays as a performer plays, which nothing saves and no undo takes back:

- `live({ min, max, default, label? })`: a number the card moves, such as an XY pad. A `Signal` in `sound`, gliding over 20 ms; it starts at its default.
- `trigger({ label? })`: a bang: a `Signal` that is 1 for the one sample after the card fires it.

## Its card

Without `card`, the card has a knob per knob and live control, a row of steps per pattern, and a button per option, toggle and trigger. For your own, give `card` in a `.tsx` file. It draws from the record and the watches and plays what it needs; it keeps no state of its own.

A card, in the row under the tracks, is at most 144 points tall: more is cut off. It is as wide as it needs, at least 200, and the row scrolls sideways. So put things side by side rather than stacked: a `Knob` is 56 by 72, so two rows of knobs fill a card; a `Pad` 120 square unless `size` says; a `Meter` 64 tall; `Steps` 14 tall; a line of text about 16.

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

The card gets `state`, the record with every field it leaves out at its default; `watches`, the last value of each watch, and draws again as they move; `update(label, change)`, which changes the record as one undo step; and `set`, `fire`, `play` and `send`, which play its live controls, triggers and voices as a performer would (`Performer` in `sdk.ts`).

## A control loop, a canvas, the keys

For more than knobs, a tool keeps a `memory` and runs a control loop:

- `memory: () => ({ ... })` makes what the tool keeps and nothing saves, per instance. The card and the loop get it, and a click may change it.
- `tick: ({ state, watches, memory, dt, time, set, fire, play, send }) => { ... }` runs about 30 times a second while the window is open, `dt` seconds apart. It plays as a performer would and moves what is in `memory`; it does not change the record. Offline, in `--render`, it does not run.
- What the loop plays at once lands up to 30 ms late. Patterns in time with the bars belong in a sequencer (see "Children"). For a rhythm the loop makes up, play ahead: `time` is the clock of the sound, in seconds, and `fire`, `play` and `send` with `{ at }` happen on the sample of `at`:

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
- `send(event, { at, child })` sends a note event (`NoteEvent` in `sdk.ts`), such as `{ type: "cc", controller: 64, value: 1 }`, the pedal down. Values are 0 to 1, the bend -1 to 1; a plugin gets any controller, 0 to 119. At most 256 wait, up to 10 s ahead: send a recording as the loop plays. `play(pitch, { hold: true })` sends only the `noteOn`.
- `onKey: (tool, { key, down }) => { ... }` hears each key of the computer keyboard going down and up, by name (`"a"`, `"1"`, `"space"`, `"left"`), after a click on its page or card, or at once on a page that opens with the window; plain keys are then the tool's, so space does not play. `onNotes: (tool, event) => { ... }` hears what reaches the tool's `notes`, not its own `send`; an instrument plays it too. `event.at` is the `time` it sounded, to the sample, so a recording sent again with `{ at }` plays as played. At the top the keyboard plays `notes`; in a track, its clips and the keyboard. Both get what `tick` gets, and `update`, which changes the record as a card does:

  ```ts
  onKey: ({ send }, { key, down }) => {
    const step = "awsedftgyhuj".indexOf(key); // a row of the keyboard as piano keys
    if (step < 0) return;
    send(down ? { type: "noteOn", pitch: 60 + step, velocity: 0.8 } : { type: "noteOff", pitch: 60 + step });
  },
  ```
- `<Canvas width height shapes background? onPress? onDrag?>` draws `shapes` (`{ kind: "circle", x, y, radius, color }`, `{ kind: "rect", x, y, width, height, color, radius? }`, `{ kind: "line", from: [x, y], to: [x, y], color, width? }`, in points from its top left) and hears the pointer: `onPress(x, y)` and `onDrag(x, y)` from 0 to 1 across and down. A card or page draws again after every tick.

## A project that is one tool

`page: (page) => ...` draws the whole window, from what a card gets and `size`, its width and height in points. Fill it: the window may be any size, and the page draws again when it changes. Keep positions from 0 to 1 and scale them by `size` as it draws.

To make the project one tool:

1. Delete `state/arrangement/`.
2. Put one record of the tool at the top: `state/<name>.json`, or `state/<name>/instance.json` for a parent.
3. Make the tool a `source`, or an `instrument` when it plays notes with `play`; a parent leaves `kind` out.
4. Connect it to the speakers in `project.json` (see `agent-docs/project-json.md`): `{"from": {"instance": "<name>", "port": "audio"}, "to": {"device_output": 0}}`.

The window shows its page, and space plays and stops. With `transport: true` in the tool it shows play, stop, the position, the tempo and the click too, and the page and cards get `transport` (`playing`, `tick`, `bar`, `beat`, `bpm`) and draw again as it moves. `{"from": {"device_input": 0}, "to": {"input": {"instance": "<name>", "port": "audio"}}}` as well plays the microphone live into its audio input: an `effect` hears it as `input`, a parent from `{ port: "audio" }`. Tell the composer to use headphones. `{"app": "Music"}` in place of `{"device_input": 0}` plays another app, and `{"app": "all"}` every app.

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

## Children: a tool that plays other tools

A tool with a `behaviour` is a parent: it has children. Its record is `state/<name>/instance.json`, and every record next to it, `state/<name>/<child>.json`, is a child of any tool: a built-in device such as `instrument.synth` or `delay` (enable its extension in `project.json`), or a tool of this project, such as a `source`. The behaviour is a pure function of the record and the children that says how they connect. A parent leaves `kind` out.

```ts
import { list, text, tool, type Connection, type Declaration } from "./sdk";

tool({
  name: "band",
  title: "Band",
  when: "You want a sound through a chain of effects that a record names",
  doc: "Plays the child `lead` through the children `effects` names, in order.",
  state: { lead: text({ default: "drone" }), effects: list(text(), { max: 4 }) },
  behaviour: ({ state: { lead, effects } }): Declaration => {
    const chain = [lead, ...effects];
    const connections: Connection[] = chain.slice(1).map((effect, index) => ({
      from: { child: chain[index] },
      to: { child: effect },
    }));
    connections.push({ from: { child: chain[chain.length - 1] }, to: { port: "audio" } });
    return { connections };
  },
});
```

- It gets `state`, every field at its default where the record leaves it out, and `children`, each `{ name, tool, state }`. A child's `state` is its record as the file holds it, so a field it leaves out may be missing. It returns `{ connections }`, each from one port to another. Give it the return type `Declaration`, as here: without it TypeScript lets a misspelled key through.
- A port of a child is `audio` when left out. Leave out `child` for a port of the parent itself: from `{ port: "audio" }` comes what reaches its audio input, from `{ notes: "notes" }` what reaches its `notes`; to `{ port: "audio" }` goes what it plays, through its own `sound` as `input`; to `{ number }`, `{ bang }` or `{ notes }` an output of that name, for its own parent.
- Put the parent at the top of a project that is one tool, or in a track's `effects`, at `state/arrangement/<track>/<name>/instance.json`, where it hears the track's sound and notes and plays into its mixer.
- The behaviour runs in Bun when the record or a child changes; its answer plays a moment later, the last one until then. `--render` and `--analyze` wait for it.
- Events connect by the key of their kind at both ends, to the sample:
  - `{ from: { child: "lfo", number: "cutoff" }, to: { child: "filter", number: "cutoff_hz" } }`: the number `lfo` sends with `output` turns the knob `cutoff_hz` of `filter` in its units, Hz: a knob of a tool of this project, or any number a lane can automate. It glides, nothing is saved, and without the connection the knob glides back. To `{ child, live }` it moves a live control instead.
  - `{ from: { child: "ears", bang: "onset" }, to: { child: "drop", trigger: "hit" } }` fires a trigger.
  - `{ from: { child: "ears", notes: "hits" }, to: { child: "synth", notes: "notes" } }` plays an instrument; `notes` is the input of each. `{ from: { notes: "notes" }, to: { child: "piano", notes: "notes" } }` plays it from the keyboard.
- A parent plays its children by name: `play` and `send` with `{ child: "synth" }` reach its `notes`; `set({ child: "lfo", live: "rate" }, 2)` and `fire({ child: "drop", trigger: "hit" })` a live control and a trigger of a tool of this project. A knob of a child is its record: a cable moves it.
- A `sequencer` child loops note patterns in time with the bars, also in a render, out of its output `notes` (`agent-docs/sequencer.md`). `set({ child: "bass", live: "launch" }, 1)` launches its second pattern at the next bar. A parent's card and page see the watches of each child as `<child>.<watch>`: a sequencer's `playing` and `queued` are a pattern's index, -1 for none, -2 for a stop that waits.
- A missing child or port, or a behaviour that throws, is a problem of the parent's record; one a performer names, of its tool. The rest plays.

A clip launcher, with a sequencer `bass`, an instrument `synth` and an effect `fx` as its children:

```ts
import { h, tool, type Declaration } from "./sdk";

tool({
  name: "loops",
  title: "Loops",
  when: "You want bass patterns to launch in time with the bars",
  doc: "Plays the patterns of the sequencer `bass` on `synth` through `fx`; the page launches them.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "bass", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "fx" } },
      { from: { child: "fx" }, to: { port: "audio" } },
    ],
  }),
  transport: true,
  page: ({ watches, set }) =>
    h("div", { style: { direction: "row", gap: 8, padding: 16 } },
      [0, 1, -1].map((pattern) => {
        const plays = watches["bass.playing"] === pattern, waits = watches["bass.queued"] === pattern;
        return h("div", {
          style: { padding: 8, radius: 6, background: plays ? "#7c3aed" : waits ? "#4c1d95" : "#2a2f3a" },
          onClick: () => set({ child: "bass", live: "launch" }, pattern),
        }, pattern < 0 ? "Stop" : `Bass ${pattern + 1}`);
      }),
    ),
});
```

## Check your work

1. `bun x tsc -p extensions` checks the types. The first run downloads TypeScript.
2. `problems.txt`, or the problems `sound-tools . --inspect` prints, list what is wrong under `extensions/<file>`: what does not load or build, and, while the window is open, the first tick or click that failed. Code that runs for more than a moment, such as a loop that never ends, stops the tools of its file until the next change.
3. Use the tool as the composer will: write its record (`"state": {}` is its defaults; an effect in a track is also named in the track's `effects`). Then measure it with `agent-docs/inspect.md`: `--analyze` shows the loudness and the bands of each moment.
