# Writing a tool for this project

A tool is TypeScript: a device on a track, the song page, or the whole project. One tool per file in `extensions/`. Import from `./sdk`; `sdk.ts` lists every name by its module of `sdk/`. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`.

## Check your work, after every change

Run 1 and 2 in `extensions/`, the rest with `sound-tools-next` on the project folder.

1. `bun check -p .`: types, token names and props.
2. `bun test`: one test for each thing the tool must do: `render(tool, self)`, then `find`, `press`, `act` and `apply`, as `song.test.tsx` does.
3. `sound-tools-next . --snap out.png --play 2`: a picture after two seconds of playing.
4. `--tree out.json`: every element with its text, and `controls` (key, label, role, value or `checked`, `expanded`, actions), `playing`, `playhead`, `meters` and `problems`. Check each control you made says its state.
5. `--act steps.json` runs actions first, on the project itself: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "fader-t00", "action": "set", "value": -6 }, { "action": "undo" }]`. `--script steps.json` points by key: `{"events": [{ "t": 0.5, "type": "tap", "key": "mute-t01" }, { "t": 1, "type": "type", "text": "-6" }, { "t": 2, "type": "drag", "key": "fader-t00", "dy": 40 }]}`.

`problems.txt` says why what does not load. `--inspect` prints the records; `--analyze` and `--render` play the sound.

## A tool

```tsx
import { boolean, h, input, list, number, tool } from "./sdk";
import { Cell } from "./ui/cell";
import { binder } from "./ui/field";
import { Knob } from "./ui/knob";
import { Toggle } from "./ui/toggle";
import { DeviceCard } from "./ui/blocks/device-card";

const fields = { steps: list(boolean({ default: true }), { max: 16 }), depth: number({ min: 0, max: 1, default: 1 }) };

export const chop = tool({
  name: "chop",
  title: "Chop",
  when: "You want a sound cut in a rhythm of sixteen steps",
  doc: "Lets the sound through on the steps that are on, `depth` how far it cuts the others.",
  state: fields,
  sound: ({ depth }) => input.times(depth.negate().plus(1)),
  view: (self, ctx) => {
    const field = binder(self, ctx, fields);
    const steps = Array.from({ length: 16 }, (_, i) => self.state.steps[i] ?? true);
    return (
      <DeviceCard id="chop" title="Chop" cells={[
        ...[0, 8].map((from) => <Cell text={`Steps ${from + 1} to ${from + 8}`} span={4}><row gap={1}>{steps.slice(from, from + 8).map((on, j) => (
          <Toggle id={`step-${from + j}`} label={`Step ${from + j + 1}`} text={`${from + j + 1}`} on={on} onToggle={() => ctx.update("Step", (draft) => void (draft.state.steps = steps.map((was, k) => (k === from + j ? !was : was))))} />
        ))}</row></Cell>),
        <Knob {...field("depth")} />,
      ]} />
    );
  },
});
```

- The next agent reads `when` and `doc`. In `sound` a number field is a `Signal`.
- To put a device on a track, add `state/song/<track>/chain/<name>.json` as `{"tool": "chop", "state": {}}`, and its name to `order` in the chain's `instance.json`.
- A tool without `view` gets the card of `ui/blocks/generic-card.tsx`.

## Views

`view: (self, ctx) => …` is a plain function of `self.state` and `self.children`, run again after every change and action. Never name a variable `h`. A part of a view types its records with `Drawn<ValuesOf<typeof fields>, typeof children>`.

- `ctx` is what `tick` gets: `update`, `send`, `play`, `clock`, `memory`.
- `memo(key, deps, () => …)` builds a part again only when one of `deps`, all it shows, changes; its keys come from `deps`.
- `memory` is never saved, such as the chosen track: change it in a handler to draw again.
- What does not fit is cut off: use `scroll` and `maxH`.

## Look: theme, primitives, components, blocks

Use the top layer that fits; each uses only those under it.

1. Theme, `extensions/theme.json`: colours, text, spacing, radius, shadows, sizes. Props name them (`fill="surface"`, `gap={2}`, `"sound/25"` at 25%); code that needs a number imports the file. `sound` (green) is sound that moves; what is chosen takes `selected`.
2. Primitives, each prop in `sdk/view.ts`: `box`, `row`, `col`, `text`, `shapes`, `field`, `child`, `live`; `hoverGroup` and `reveal` (shown on hover: the quiet rule), `overlay` (menus, lists, tips), `group` (one Tab stop).
3. Components, `extensions/ui/`: Button, Toggle, Segmented, Select, Menu, Popover, Tooltip, Knob, Slider, Fader, TypeIn, Meter, Pad, TextField, Cell, Display, Kbd, Notice, Dot; `value.ts` steps, clamps and reads typed values.
4. Blocks, `extensions/ui/blocks/`: DeviceCard, `generic-card.tsx` (its `card`), Panel, Section, StepRow, StepGrid, Keyboard, PaintGrid, EmptyState.

Change them, add yours. `--main gallery --snap g.png --size 1470x1500` shows them all.

- A device is a `DeviceCard`: a display, then cells, two to a column, top to bottom. It grows wider, never taller.
- Controls are 24, 28 or 32 points high (`size`); a card's are `sm`.
- Use the app's transport: add no Play button.
- Every control takes the keys: `actions` (`increment`, `decrement`, `set`, `reset`); many go in a `group`.

## What moves every frame

A live value is an expression the window works out every frame, without the view. Start from `live` (`playhead`, `playing`, `elapsed`, `peak(path)`, `field(name)`, `sounding(pitch)`, `drag(key)`) and combine with `Live`'s methods in `sdk/values.ts`: arithmetic, comparisons, `decibels`, `hold`, `fall`, `smooth`.

- `<text live={live.peak("t01/mix").hold(3).decibels()} unit="db" />` shows one, with `unit` or `digits`.
- `at`, `w`, `h`, `opacity` and a `shapes`'s `trim` take one, as `ui/meter.tsx` and `ui/knob.tsx` do.
- Code cannot read one: read the clock in `tick` or a handler.
- `binder(self, ctx, fields)` of `ui/field.ts` wires a number's control: range, unit, default, change, and what a lane plays (`held`), which `render(tool, self, { held })` tests.

## Changes and actions

- `update(label, (draft) => …)` changes records: write to `draft`. All the changes of one press, action, drag or paint are one step of undo.
- An element a pointer or an agent uses has a unique `key`, a `label` ("Mute Track 2") and its state (`checked`, `value`). Handlers: `onPress`, `onDragEnd`, `onEnter` (a paint: each element a held press enters), `onCommit` of a `field`, `actions`.
- `live.drag(key)` follows a `drag`, such as a thumb's `at`; `onDragEnd` hears where it stopped.
- `tick` runs about 30 times a second.
