# Writing a tool for this project

When no tool does what the composer asks, write one in TypeScript. It plays as soon as you save the file, with no build, and the runtime writes a doc for the next agent. A tool takes one of two shapes:

- **A device in a track**: an effect or an instrument, used in the arrangement as a built-in one is. Its records go in tracks, and its view is a card in the row under the tracks.
- **The project is the tool**: an experiment, a game, a thing to play live. There is no arrangement: one record at the top, and its view fills the window. See "A project that is one tool".

To play a track from the computer keyboard, write nothing: with the computer keys on (cmd-K, or the keyboard button in the transport), the letter rows play the selected track as a MIDI keyboard does.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own view. Make the folder, then run `sound-tools . --inspect` once: the runtime writes `extensions/sdk.ts`, the SDK with every type and function, `bun.d.ts`, the Bun calls tools make most, and `tsconfig.json`. Do not edit them; read `sdk.ts` when you need the exact types. A tool needs no entry in `project.json`.

The runtime runs them with Bun, which the app ships and puts on your `PATH`. While the app has the project open, a save of code or JSON in `extensions/` starts the tools again in a moment: every file and helper loads again, every `memory` starts over, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

## A tool

```ts
import { input, number, phasor, sin, TAU, tool } from "./sdk";

tool({
  name: "wobble",
  title: "Wobble",
  when: "You want the volume of a sound to pulse",
  doc: "A tremolo: the volume moves up and down `rate` times a second. `depth` is how far it dips, 0 not at all, 1 to silence.",
  state: {
    rate: number({ min: 0.1, max: 20, default: 4, unit: "hz" }),
    depth: number({ min: 0, max: 1, default: 0.5 }),
  },
  sound: ({ rate, depth }) => {
    const wave = sin(phasor(rate).times(TAU)).times(0.5).plus(0.5);
    return input.times(depth.times(wave).negate().plus(1));
  },
});
```

- `name`: the `tool` of its records and the name of its doc. Lowercase letters, digits, `-` and `_`, and not the name of a built-in tool.
- `title`: what its view and the pickers say.
- `when` and `doc`: what the next agent reads in the map and in `agent-docs/<name>.md`. Say what the tool does to the sound and what each field does musically, also between its ends: is the middle of a knob half as much, or less? The runtime adds where the record goes and a table of the fields, so do not repeat those.
- `state`: the fields of its record. A field name is lowercase letters, digits and `_`.
- `inputs`: what plays it and nothing saves, see below.
- `sound`: the sound, as a `Signal`. Left out, it passes `input` through. A sound that reads `input` hears what comes in: in a track it goes in the `effects`. One that does not is what a track plays, its `instrument.json`: a drone, a texture, a generative part, or notes.
- `voices`: how many notes the sound plays at once. 1, when left out: one voice that runs all the time, notes or not, and follows the newest held note. Up to 8 for an instrument that plays chords: the sound then runs once per note.

## The sound

`sound` gets the fields and inputs, and returns a `Signal`: a stream of samples, one value per sample, run for each channel apart and for each voice apart. It is called when the tool loads, once for each combination of choices (at most 256), to build the sound; the sound then plays by itself. So a number field is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

Combine signals with methods, which take a signal or a number: `.plus`, `.minus`, `.times`, `.over`, `.mod`, `.negate()`, and the comparisons `.lt`, `.gt`, `.le`, `.ge`, `.eq`, `.ne`, which give 1 for true and 0 for false. And with these functions, all in `sdk.ts` with their docs:

| | |
| --- | --- |
| What comes in | `input`, `channel` (0 left, 1 right; 0 in a stereo sound), `sampleRate`; `inputLeft`, `inputRight` for a stereo sound |
| The note of a voice | `note.freq` (Hz, with bend), `note.pitch` (MIDI, 69 is A4), `note.gate` (1 while held), `note.velocity` (0 to 1), `note.onset` (1 in its first sample) |
| The piece | `beat` (quarter notes from the start, while playing), `bpm`, `playing` |
| Math | `sin`, `cos`, `tan`, `tanh`, `abs`, `sqrt`, `exp`, `log`, `floor`, `wrap` (the part after the point), `min`, `max`, `pow`, `clamp`, `mix(a, b, amount)`, `db(decibels)`, `saturate`, `PI`, `TAU` |
| Oscillators | `sine(hz)`, `saw(hz)`, `square(hz, width?)`, `triangle(hz)`, from -1 to 1. `phasor(hz)`: a ramp 0 to 1 to build others. `noise()`: white noise. `mtof(pitch)`: the Hz of a MIDI pitch |
| Time | `delay(x, ms, longest?)` up to 4000 ms, smooth when `ms` moves (chorus, tape wobble); give `longest` with more voices than one. `smooth(x, ms)`. `adsr(gate, attack, decay, sustain, release)` in ms |
| Filters | `lowpass`, `highpass`, `bandpass(x, hz, q?)` |
| Events | `rise(x)`: 1 where `x` goes above 0, a clock: `rise(wrap(beat.times(4)).lt(0.5))` ticks every sixteenth. `change(x)`, `hold(x, when)`: sample and hold |
| Tables | a list of numbers such as a `pattern`, a `file` field and a `buffer`: `.at(index)` (whole part, wrapped), `lookup(table, phase)` (0 to 1 over the table, smooth), `.length`. A file plays at its own speed with `lookup(sound, phasor(sampleRate.over(sound.length)))`; grains read short stretches of it |

A sound that returns one signal runs on each channel apart. One that returns `{ left, right }` runs once for both and hears both through `inputLeft` and `inputRight`, as a ping-pong delay does.

A signal used twice is one: `const lfo = phasor(2)` heard in two places is one oscillator with one memory; call `phasor(2)` twice for two.

Inside `sound` only:

- `feedback()`: a value that feeds back. Read it as a signal; it gives what it was `.set(...)` to one sample before. Set it once.
- `buffer(seconds)`: memory to `.write(index, value)` every sample and read with `.at` or `lookup`, for loops and grains.
- `output(name, ...)`: sends out what a parent connects to another child (see "Children"): a signal as a number, once per 64 samples, which its view also reads as `now.value(name)`: a meter, a step light; `{ bang: when }`, a bang on the sample `when` goes above 0, such as an onset; `{ gate, pitch, velocity? }`, notes: one starts where `gate` goes above 0, at the MIDI `pitch` and the `velocity` (0 to 1) of that sample, and ends where the gate falls or the pitch moves. Up to 16 numbers and 8 of the others; a sound of more voices than one sends only numbers, its newest note's.

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

## Built-in devices

A sound plays the built-in effects through `devices`: `devices.reverb`, `devices.filter`, `devices.compressor` and the others `sdk.ts` lists. Each takes a signal and settings, and gives what it plays as a signal:

```ts
import { beat, devices, input, sin, TAU, tool } from "./sdk";

tool({
  name: "swept-room",
  title: "Swept room",
  when: "You want a filter that sweeps once a beat, into a room",
  doc: "A low pass that opens and closes once a beat, then the built-in reverb, whose numbers are this tool's.",
  state: { ...devices.reverb.fields },
  sound: (fields) => {
    const sweep = sin(beat.times(TAU)).times(0.5).plus(0.5);
    const swept = devices.filter(input, { cutoff_hz: sweep.times(4000).plus(300) });
    return devices.reverb(swept, fields);
  },
});
```

- A number of a device takes a number, a field or any signal, such as an LFO, which moves it once per 64 samples. The rest of its record is written as it is: `devices.filter(x, { type: "high_pass" })`. `sdk.ts` types each with its range, unit and options, and a value the device refuses is a problem of the tool as it loads.
- `devices.reverb.fields` are its numbers as `number` fields, with their ranges and defaults. Spread them into `state` and hand the fields on, as above: the view, automation and the record treat them as any number. `prefixed("far_", devices.reverb.fields)` names those of a second one.
- What a device plays is of the channel the sound runs on. A sound that returns `{ left, right }` reads `.left` and `.right`, and gives a device `{ left, right }`.
- The engine waits for the latency of a device, so what goes round it stays in time with it.
- Not in a sound of more voices than one, where each voice would play one, and not inside a `feedback`, as a device hears a block at once. A plugin, or a device with a record of its own, is a child: see "Children".

## Fields

Whatever reads a field follows it: a sound that reads a number glides to it at once, a behaviour or a view that reads it runs again.

| Field | Record holds | In `sound` |
| --- | --- | --- |
| `number({ min, max, default?, integer?, unit?, label? })` | a number; `default` is `min` when left out | a `Signal` that glides; a lane or a cable moves it unless `integer` |
| `boolean({ default, label? })` | `true` or `false` | a `Signal`, 1 or 0 |
| `choice({ options, default, label? })` | one of `options` | the option itself: a change switches to the sound of that option, fading in over 10 ms |
| `text({ default?, label? })` | text | nothing |
| `file({ label? })` | the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound: one channel at the engine's rate, up to 60 s, once the file is there |
| `list(item, { max?, default?, label? })` | a list of `item`: a field, or an object of fields such as `{ name: text(), notes: notes() }` | a list of numbers is a table: `.at(index)` |

`pattern({ length, min, max, default })` is a list of `length` numbers, such as the steps of a sequence. `notes()` is a list of `noteFields`: `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`, times in ticks (960 a quarter). `unit` is `"hz"`, `"ms"`, `"db"` or `"percent"`: a view shows it, a knob in `"hz"` turns on a log scale, and one in `"percent"` still holds 0 to 1. A sound reads at most 32 numbers and booleans.

## Inputs

What plays the tool as a performer plays, which nothing saves and no undo takes back. Each is a `Signal` in `sound`:

- `number({ min, max, default?, label? })`: a number its view, its code or a cable moves, such as an XY pad. It glides over 20 ms and starts at its default.
- `bang({ label? })`: a `Signal` that is 1 for the one sample after it is sent.

Every sound also has the input `notes`, which plays its voices. `send` plays an input: `send("rate", 2)`, `send("hit")`, `send("notes", { type: "noteOn", pitch: 60, velocity: 0.8 })`.

## Its view

Without `view`, it gets a knob per number field and number input, a row of steps per list of numbers, a chooser per file, and a button per option, boolean and bang. For your own, give `view` in a `.tsx` file. It runs once and makes what shows; what it keeps goes in `memory`. The window gives it a room, `size`: a card in the row under the tracks, or the whole window for a tool at the top.

A card is as tall as its room, 144 points: more is cut off. It is as wide as it needs, at least 200, and the row scrolls sideways. So put things side by side rather than stacked: a `Knob` is 56 by 72, so two rows of knobs fill a card; a `Pad` 120 square unless `size` says; a `Meter` 64 tall; `Steps` 14 tall; a line of text about 16.

```tsx
import { Knob, Meter, Pad, Steps, h, type Style, tool } from "./sdk";

const column: Style = { gap: 8 };
const row: Style = { direction: "row", gap: 8, align: "center" };
const button: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };

tool({
  // name, title, when, doc, state, inputs and sound
  view: ({ send }) => (
    <div style={column}>
      <div style={row}>
        <Knob value="cutoff" label="Cutoff" min={100} max={8000} default={1200} unit="hz" />
        <Pad x="bend" y="drive" size={72} />
        <Meter value="level" />
      </div>
      <div style={row}>
        <Steps path="steps" playing="step" />
        <div style={button} onPress={() => send("hit")}>Hit</div>
      </div>
    </div>
  ),
});
```

| Element | Does |
| --- | --- |
| `<div style onPress>` | A box; `Style` in `sdk.ts` lists every style. Text goes inside. `onPress` hears a click, as a `Press`. |
| `<Knob value label min max default unit? played?>` | Turns the number `value`, as `now.value` names it: a field, whose drag is one undo step, or a number input. With `played` it moves with a lane. |
| `<Steps path max? playing?>` | A row of steps on a list of numbers: a click turns a step on (to `max`, the item's max by default) or off (to its min). `playing` names an output whose value is the step that lights up. |
| `<Meter value label?>` | A bar that shows a number from 0 to 1, such as an output. |
| `<Pad x y size?>` | A square for the pointer: across moves the number input `x`, up moves `y`. |

All code of a tool gets the same (`Context` in `sdk.ts`): `state`, the record with every field it leaves out at its default; `children`, the records of its children; `outputs`, the last value of each number output of its sound and of its children's; `time` and `transport` (`playing`, `tick`, `bar`, `beat`, `bpm`); `memory`; `now`, see "Shapes that move"; `update`, see "Changing records"; `openFile()`, which asks the composer for a file, copies it into `assets/audio/` and gives its name for a `file` field; and `send` and `play`, which play its inputs and voices as a performer would.

### Values that change

A view and a component run once. A value that changes (a field, a child, an output, `transport`, `memory`, `size`) goes in a function, which the window runs again when what it read changes, and only what it gives changes on screen: `style={() => (state.on ? lit : plain)}`, and `{() => `${state.notes.length} notes`}` as a child. Read such a value in the body itself and the view fails with an error that says how: `x={note.start}` is `x={() => note.start}`. A handler reads values as it runs, so it needs no function around them.

- `<For each={() => state.notes}>{(note, index) => ...}</For>` makes a row per item once and keeps it while its item stays: a moved note changes only what reads it. `index()` follows its place. Its body runs once too: `x={() => note.start}`.
- `<Show when={() => memory.open}>{() => ...}</Show>` makes its part each time it shows.
- `memo(() => ...)` works a value out once for many functions. `selector(() => memory.selected)` gives `isPicked(key)`, which changes only for the keys that come or go, so a click selects one note at the cost of one.
- Set a field of `memory` to change it, from any code. A `Set` or a `Map` is replaced, never changed in place: its type refuses `.add`.

## Shapes that move

A `<Space>` draws shapes: `<Rect>`, `<Circle>`, `<Line>`, `<Arc>`, `<Label>` (text) and `<Group>`. It has units of its own, such as ticks across and notes down (`unit` is points per unit; a point without one), scrolls over `extent` and zooms in `zoom` by itself, and draws only what shows. The knob, steps, meter and pad above are spaces.

What moves on a space moves in the window with no code running, so it keeps up with the pointer and the playhead: give a number a live value from `now` in place of a number. `now.tick` is where the piece plays, `now.value(name)` a number by its name (a field as a drag turns it, a number input, an output; `"bass/synth.cutoff_hz"` a child's), and `now.drag(id)` how far the drag `id` went, rounded to its `snap`. Combine them with `.plus`, `.times`, `.snap` and the rest; a function may give one. Code hears a drag once, at `onEnd`, and what moved stays where it is until what its code changes then arrives:

```tsx
const roll = { unit: { x: 0.1, y: 12 }, extent: { x: [0, 38400], y: [0, 128] }, zoom: { x: [0.1, 10] }, scroll: { y: 40 } } as const;

view: ({ state, memory, update, now }) => {
  const isPicked = selector(() => memory.selected);
  // One drag for every note: a drag many shapes share crosses once.
  const drag: Drag = {
    id: "move", snap: { x: 240, grid: true }, axis: "x",
    onEnd: ({ dx }) => update("Move notes", ({ state }) => {
      state.notes = state.notes.map((note, index) => (memory.selected.has(index) ? { ...note, start: note.start + dx } : note));
    }),
  };
  return (
    <Space key="roll" width={800} height={400} {...roll}>
      <For each={() => state.notes}>
        {(note, index) => (
          <Rect x={() => note.start} y={() => 127 - note.pitch} width={() => note.length} height={1} color="#a9b1ff"
            moves={() => (isPicked(index()) ? ["move"] : undefined)} drag={drag} onPress={() => { memory.selected = new Set([index()]); }} />
        )}
      </For>
      <Line from={[now.tick, 0]} to={[now.tick, 128]} color="#7ee0a0" width={2} />
    </Space>
  );
},
```

- `moves` names the drags a shape moves with, `grows` those that make it wider and taller, as the selected notes above: one small change when the selection does. A `<Group x y>` places its shapes from `x`, `y`, and moves them with its own `moves`; with a `clip` of `{ width, height }` they do not show or hear the pointer outside that box: a clip and its notes, which end at its end.
- `snap` with `grid: true` puts the pressed shape on the grid, wherever it started; `anchor` names the point that lands there, such as `{ x: end }` on the handle at the end of a note. Escape lets go of a drag, and its code hears nothing.
- `follow: { x: "roll" }` shows the scroll and zoom of the space `roll` across, and scrolls it: a ruler, or a lane under it. `scroll` is where a space starts. On a `div`, `follow: { y: "timeline" }` scrolls its children with it, cut off at its edges: the heads of the rows of a timeline.
- `<>...</>` gives several children where one goes, such as a component that gives two shapes; import `Fragment` with `h`.
- A drag can also turn a number as it goes, with no code, as a knob does: a drag of `{ y: { value: "cutoff" } }`, named as `now.value` names it; for a field, also a child's, the whole drag is one undo step. `onPress` hears a `Press`: where in units, the button, the clicks and the modifiers. `onMove` runs code at every move, a moment behind the pointer: only for drawing with the pointer.

Levels, the clock and easing are live values too, so a meter or a light moves on every frame with no code. Paths such as `"bass/synth"` reach a record deeper down:

- `now.peak(path, channel?)`: the loudest sample since the last frame of a `meter` child (`agent-docs/meter.md`) by its path, `"bass-meter"` or `"bass/meter"`, which hears what its parent connects to it; `"child.name"`, peaks a child keeps. `now.devicePeak()` is the device's.
- `now.value(name, { played: true })`: a field as it plays, what a lane or a cable plays into it, else its record. A drag of `{ y: { value: "bass-mix.gain_db" } }` turns a child's.
- `now.elapsed`, seconds, and `.mod`, `.pow`, `.decibels()`. `.smooth(ms)`, `.fall(perSecond)` and `.hold(seconds)` follow a value over frames: a meter's bar is `level.fall(0.5)`, its peak line `level.hold(1.5).fall(0.5)`.
- `blend(from, to, by)` is a colour between two: `blend("#3a1418", "#ff4d5e", now.elapsed.mod(1).lt(0.5))` blinks.
- `<Wave list>` draws a list across its box: `now.scope(path)`, the last sound of a meter, or `now.history(value, frames)`, the last values of any live value. `band` draws a waveform.

## A control loop, the keys

For more than knobs, a tool keeps a `memory` and runs a control loop:

- `memory: () => ({ ... })` makes what the tool keeps and nothing saves, per instance. All its code gets it and may change it.
- `tick: ({ state, memory, dt, time, play, update }) => { ... }` runs about 30 times a second while the window is open, `dt` seconds apart. Offline, in `--render`, it does not run.
- What the loop plays at once lands up to 30 ms late. Patterns in time with the bars belong in a timeline (see "Children"). For a rhythm the loop makes up, play ahead: `time` is the clock of the sound, in seconds, and `play`, and `send` of a bang or notes, with `{ at }` happen on the sample of `at`:

  ```ts
  tick: ({ memory, time, play }) => {
    if (memory.next < time) memory.next = time; // after a pause, start from now
    while (memory.next < time + 0.1) {
      play(60, { at: memory.next });
      memory.next += 0.25;
    }
  },
  ```

- `send("notes", event, { at })` sends a note event (`NoteEvent` in `sdk.ts`), such as `{ type: "cc", controller: 64, value: 1 }`, the pedal down. Values are 0 to 1, the bend -1 to 1; a plugin gets any controller, 0 to 119. At most 256 wait, up to 10 s ahead: send a recording as the loop plays. `play(pitch, { hold: true })` sends only the `noteOn`.
- `onKey: (tool, { key, down, shift, cmd, alt, ctrl }) => { ... }` hears each key of the computer keyboard going down and up, by name (`"a"`, `"1"`, `"space"`, `"left"`), after a click on its view, or at once on a view at the top that opens with the window: those `keys: ["delete", "cmd-d"]` names, the rest staying the window's, so space still plays; without `keys`, every plain key. A key with cmd, ctrl or alt only goes down, and one the window uses, such as cmd-z, stays the window's. `onNotes: (tool, event) => { ... }` hears what reaches the tool's `notes`, not its own `send`; its sound plays it too. `event.at` is the `time` it sounded, to the sample, so a recording sent again with `{ at }` plays as played. The MIDI keyboard plays `notes`, and in a track its clips too.

## Changing records

`update(label, change)`, from any of a tool's code and also after an `await`, changes its record and its children's: `change` runs at once on `{ state, children }`, copies of the latest, every field at its default where the file leaves it out, children's too. Edit a field or a child's `state` (typed when the parent names its `children`, else `as TimelineState`), push a new child, or leave one out of a new list. A child's `name` is the id of its record and stays: keep a title in a field. Only what changes lands, on the records as they are then; a record its tool refuses fails the whole update, in `problems.txt`. Updates of one label in a row are one undo step, saved once they stop and every 5 s while they go on, so a `tick` may update every frame.

```ts
update("Add a bass line", ({ children }) => {
  const bass = children.find((child) => child.name === "bass")!.state as TimelineState;
  bass.notes = [...(bass.notes ?? []), { start: 0, length: 480, pitch: 36, velocity: 100 }];
});
```

## A project that is one tool

At the top of a project, `view` draws the whole window: its `size` is the window's room in points. Fill it: the window may be any size, so read `size` in a function, `width={() => size.width}`, or give the top `div` and what fills it `grow`.

To make the project one tool:

1. Delete `state/arrangement/`.
2. Put one record of the tool at the top: `state/<name>.json`, or `state/<name>/instance.json` for a parent.
3. Give it a `sound`, with `voices` when it plays chords with `play`.
4. Connect it to the speakers in `project.json` (see `agent-docs/project-json.md`): `{"from": {"instance": "<name>", "port": "audio"}, "to": {"device_output": 0}}`.

The window shows its view, and space plays and stops. With `transport: true` in the tool it shows play, stop, the position, the tempo and the click too, and what reads `transport` follows it. `{"from": {"device_input": 0}, "to": {"input": {"instance": "<name>", "port": "audio"}}}` as well plays the microphone live into its audio input: its sound hears it as `input`, a parent's from `{ port: "audio" }`. Tell the composer to use headphones. `{"app": "Music"}` in place of `{"device_input": 0}` plays another app, and `{"app": "all"}` every app.

```tsx
import { bang, Circle, feedback, h, lowpass, max, noise, number, Space, tool } from "./sdk";

type Ball = { x: number; y: number; speed: number };

tool({
  name: "rain",
  title: "Rain",
  when: "You want rain drops you can drop with the pointer",
  doc: "Each drop that lands clicks; a press drops one where it is.",
  state: {},
  inputs: { drop: bang(), bright: number({ min: 200, max: 8000, default: 2000 }) },
  memory: () => ({ balls: [] as Ball[] }),
  sound: ({ drop, bright }) => {
    const level = feedback();
    level.set(max(drop, level.times(0.995)));
    return lowpass(noise(), bright).times(level);
  },
  tick: ({ memory, dt, send }) => {
    for (const ball of memory.balls) {
      ball.speed += 2 * dt;
      ball.y += ball.speed * dt;
      if (ball.y > 1) {
        ball.y = 1;
        ball.speed *= -0.5;
        send("drop");
      }
    }
  },
  // Positions from 0 to 1, scaled by the room as it is now.
  view: ({ memory, size }) => (
    <Space grow onPress={({ x, y }) => memory.balls.push({ x: x / size.width, y: y / size.height, speed: 0 })}>
      {() => memory.balls.map((ball) => <Circle x={ball.x * size.width} y={ball.y * size.height} radius={5} color="#60a5fa" />)}
    </Space>
  ),
});
```

## Children: a tool that plays other tools

A tool with a `behaviour` is a parent: it has children. Its record is `state/<name>/instance.json`, and every record next to it, `state/<name>/<child>.json`, is a child of any tool: a built-in device such as `instrument.synth`, `delay` or a `plugin` (enable its extension in `project.json`: `plugin-host` for a plugin), or a tool of this project, or one with only `state`: data, such as a take a recorder saves. The behaviour is a pure function of the record and the children that says how they connect. A parent has one voice.

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

- It gets `state`, every field at its default where the record leaves it out, and `children`, each `{ name, tool, state }`. A child's `state` is its record as the file holds it, so a field it leaves out may be missing. It returns `{ connections }`. Give it the return type `Declaration`, as here: without it TypeScript lets a misspelled key through and blames the whole behaviour, not the wrong connection.
- A parent whose children are fixed names them and their tools in `children`, as `loops` below does: a `const` with `as const`, and the return type `Declaration<typeof children>`. TypeScript then checks each connection, `play`, `send` and child `state` against the child's tool. For a tool of this project, name what `tool` returns: `export const drone = tool({ ... })` in its file, then `import { drone } from "./drone"` and `{ lead: drone, fx: "utility" }`. Its `state`, numbers and inputs are then checked; its outputs are not, as its `sound` makes them. A child of another tool, a missing one or one it does not name is a problem of the parent's record; `children: {}` allows none.
- A child may be a parent too, at `state/<name>/<child>/instance.json`. Each child's `children` holds its own, at any depth, typed by the `children` its tool names, and `update` edits, adds (with their `children`) and takes out records at any depth; one taken out takes all it holds. `"*"` in `children` stands for any other name: `{ "*": track }` is any number of tracks.
- A port of a child is `audio` when left out. Leave out `child` for a port of the parent itself: from `{ port: "audio" }` comes what reaches its audio input, from `{ notes: "notes" }` what reaches its `notes`; to `{ port: "audio" }` goes what it plays, through its own `sound` as `input`; to `{ number }`, `{ bang }` or `{ notes }` an output of that name, which its own parent connects as any port of a child.
- Put the parent at the top of a project that is one tool, or in a track's `effects`, at `state/arrangement/<track>/<name>/instance.json`, where it hears the track's sound and notes and plays into its mixer.
- The behaviour runs in Bun when the record or a child changes; its answer plays a moment later, the last one until then. `--render` and `--analyze` wait for it.
- Events connect by the key of their kind at both ends, to the sample:
  - `{ from: { child: "lfo", number: "cutoff" }, to: { child: "filter", number: "cutoff_hz" } }`: the number `lfo` sends with `output` turns the number `cutoff_hz` of `filter` in its units, Hz: a number field of a tool of this project, or any number a lane can automate. It glides, nothing is saved, and without the connection the field glides back. To a number input, it moves that input.
  - `{ from: { child: "ears", bang: "onset" }, to: { child: "drop", bang: "hit" } }` sends a bang input.
  - `{ from: { child: "ears", notes: "hits" }, to: { child: "synth", notes: "notes" } }` plays an instrument; `notes` is the input of each. `{ from: { notes: "notes" }, to: { child: "piano", notes: "notes" } }` plays it from the keyboard.
- A parent plays its children by name: `play(pitch, { child: "synth" })` and `send({ child: "synth", notes: "notes" }, event)` reach its `notes`; `send({ child: "lfo", number: "rate" }, 2)` and `send({ child: "drop", bang: "hit" })` its inputs. A number field of a child is its record: a cable moves it.
- A `timeline` child plays notes in time out of its output `notes` (`agent-docs/timeline.md`): once at their place in the song, one per clip, so moving a clip changes one file; or round after round with `"loop": true`. `send({ child: "bass", number: "launch" }, 1)` starts it at the next bar, `0` stops it there. A parent's view reads the outputs of each child as `<child>.<name>`: a looping timeline's `playing` is 1 while it plays.
- A missing child or port, or a behaviour that throws, is a problem of the parent's record; one a performer names, of its tool. The rest plays.

A clip launcher, with two looping timelines `low` and `high`, an instrument `synth` and a utility `fx` as its children:

```ts
import { h, tool, type Declaration } from "./sdk";

const children = { low: "timeline", high: "timeline", synth: "instrument.synth", fx: "utility" } as const;
const loops = ["low", "high"] as const;

tool({
  name: "loops",
  title: "Loops",
  when: "You want bass loops to launch in time with the bars",
  doc: "Loops the timelines `low` and `high` on `synth` through `fx`; its view starts one and stops the other, or stops both, at the next bar.",
  state: {},
  children,
  behaviour: (): Declaration<typeof children> => ({
    connections: [
      { from: { child: "low", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "high", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "fx" } },
      { from: { child: "fx" }, to: { port: "audio" } },
    ],
  }),
  transport: true,
  view: ({ outputs, send }) => {
    const launch = (chosen?: string) => {
      for (const loop of loops) send({ child: loop, number: "launch" }, loop === chosen ? 1 : 0);
    };
    return h("div", { style: { direction: "row", gap: 8, padding: 16 } },
      [...loops, undefined].map((loop) => {
        const plays = () => loop !== undefined && outputs[`${loop}.playing`] === 1;
        const waits = () => loop !== undefined && outputs[`${loop}.queued`] === 1;
        return h("div", {
          style: () => ({ padding: 8, radius: 6, background: plays() ? "#7c3aed" : waits() ? "#4c1d95" : "#2a2f3a" }),
          onPress: () => launch(loop),
        }, loop ?? "Stop");
      }),
    );
  },
});
```

Its timelines, `state/loops/low.json` and `state/loops/high.json`, loop a bar each, such as `{"tool": "timeline", "state": {"length": 3840, "loop": true, "plays": false, "notes": [...]}}`: `"plays": false` waits for a launch.

## Check your work

1. `bun check -p extensions` checks the types.
2. `problems.txt`, or the problems `sound-tools . --inspect` prints, list what is wrong under `extensions/<file>`: what does not load or build, and, while the window is open, the first tick or click that failed. Code that runs for more than a moment, such as a loop that never ends, stops the tools of its file until the next change.
3. Use the tool as the composer will: write its record (`"state": {}` is its defaults; an effect in a track is also named in the track's `effects`). Then measure it with `agent-docs/inspect.md`: `--analyze` shows the loudness and the bands of each moment.
