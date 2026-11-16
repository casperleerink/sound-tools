# Writing a tool for this project

Write a tool in TypeScript, with no build: a device on a track, a change of the song page, or the whole project as one tool. One tool per file in `extensions/`, `.tsx` with a view. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`, which the runtime writes. A save while the app is open loads the tools again.

## A tool

```tsx
import { boolean, input, list, number, tool } from "./sdk";
import { h, view } from "./sdk/view";
import { Knob } from "./ui/knob";
import { Toggle } from "./ui/toggle";

export const chop = tool({
  name: "chop",
  title: "Chop",
  when: "You want a sound cut in a rhythm of sixteen steps",
  doc: "Lets the sound through on the steps that are on, `depth` how far it cuts the others.",
  state: { steps: list(boolean({ default: true }), { max: 16 }), depth: number({ min: 0, max: 1, default: 1 }) },
  sound: ({ depth }) => input.times(depth.negate().plus(1)),
});

export const chopView = view(chop, (self, { change }) => (
  <col gap={2}>
    <row gap={1}>
      {Array.from({ length: 16 }, (_, i) => {
        const on = self.state.steps[i] ?? true;
        const flip = () => change((draft) => void (draft.state.steps = Array.from({ length: 16 }, (_, j) => (j === i ? !on : (self.state.steps[j] ?? true)))), `Step ${i + 1}`);
        return <Toggle id={`step-${i}`} label={`Step ${i + 1}`} letter={`${i + 1}`} on={on} color="green" onToggle={flip} />;
      })}
    </row>
    <Knob id="depth" label="Depth" value={self.state.depth} min={0} max={1} onChange={(depth) => change((draft) => void (draft.state.depth = depth))} />
  </col>
));
```

- `name` is no built-in tool's. `tool` describes the record (`state`), its `sound` and, for a parent, its `children` and `behaviour`; `sdk.ts` lists every name with its doc. `when` and `doc` are what the next agent reads. A number field is a `Signal` in `sound`, never a number: turning a knob does not build the sound again.
- To put a device on a track, add its record to the track's chain, `state/song/<track>/chain/<name>.json` as `{"tool": "chop", "state": {}}`, and its name to `order` in the chain's `instance.json`. The chosen track's panel shows the chain in that order.
- A tool without `view` gets a card with a knob per number, a switch per boolean and segments per choice.

## Views

`view(tool, (self, ctx) => …)` is a plain function of the records, `self.state` and `self.children`, run again after every change: no hooks, no signals. Never name a variable `h`: JSX calls `h`.

- `row`, `col`, `box`: layout (`w`, `h` in points or `"full"`, `grow`, `pad`, `px`, `py`, `gap`, `align`, `justify`, `at` to place it, `inset` to pin it, `clip`, `scroll`) and look (`fill`, `border`, `borderSides`, `radius`, `shadow`, `opacity`).
- `text`: `style`, `color`, `truncate`.
- `shapes`: `lines` and `rects`. With `units` they are in your own units (ticks, pitches) and stretch to `w` × `h`.
- `child path="t00/synth"`: the card of a child, its own view or its fields.
- `memo(key, deps, () => …)`: a part built again only when a dep changes; `deps` lists all it shows.
- `ctx.memory`: what the view keeps while the window is open, never saved, such as the chosen track. Change it in a handler; give its start as `view(tool, draw, { memory: () => ({ … }) })`.

## Look

Colours, text styles, spacing, radius and shadows are names from `extensions/theme.json` (`fill="surface"`, `gap={2}`, `radius="md"`, `style="label"`); a wrong name is a type error. `"green/25"` is a colour at 25%. Change the look of everything in `theme.json`. Components are files in `extensions/ui/` (button, toggle, knob, fader, meter): read them, change them, add your own. Follow their rules: a value is the one bright line (`text`), green is sound, a toggle that is on is tinted and hollow when off.

## Changes, actions and what moves

- `ctx.change((draft) => …, label)` changes records: write to `draft.state` and `draft.children` as to the records. It is one step of undo, called `label`. Read the value to change from `draft` or from `self`.
- An element a pointer or an agent uses has a `key`, unique in the view, and a `label` that says what it is ("Mute Track 2"), and declares what it does once: `onPress`, `actions={{ toggle, set, increment, decrement }}`, `role` (`button`, `switch`, `slider`, `tab`), and its state, `checked` or `value`. The window lists every such control for an agent, and runs its actions by label or key. A missing label is a type error.
- A `drag={{ axis, min, max }}` moves the element in the window while the pointer moves; `onDragEnd({ dx, dy })` hears where it stopped. Nothing crosses to your code while it moves.
- What moves every frame is bound by name, never set from code: `x={{ live: "playhead", scale, offset }}` (ticks), `level="meter:t00/mix"` (the level of a child, 0 to 1), `<text live="time">`.

## Check your work

In `extensions/`:

- `bun check -p .` checks types: token names, props, a press without a key or a label.
- `bun test` runs `*.test.tsx`. `render(view, self)` runs a view; `find(keyOrLabel)` gives an element; `press(key)` and `act(keyOrLabel, action, value)` give the changes they ask for as `[path, value]` and run the view again; `apply(self, changes)` gives the records after them. `song.test.tsx` shows each.

With the app's program, `sound-tools-next` (`$SOUND_TOOLS_RUNTIME` when set), on the project folder:

- `--snap out.png`: a picture of the window. Look at it.
- `--tree out.json`: every element, and `controls`: each control's key, label, role, value or `checked`, and actions; also `playing`, `playhead`, `meters` and `problems`.
- `--act steps.json`: runs actions before the picture, on the project itself, as a click would: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "fader-0", "action": "set", "value": -6 }, { "action": "undo" }]`. Check the records after.
- `--play 2`: plays two seconds from the start before the picture or the tree.
- `--script steps.json`: pointer and keys by element key, such as `{"events": [{ "t": 0.1, "type": "tap", "key": "mute-1" }]}`.
- `--inspect` prints the records; `--analyze` and `--render` play the sound.

Read `problems.txt` after each change: what does not load says why there.
