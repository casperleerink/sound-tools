# Writing a tool for this project

A tool is TypeScript, with no build: a device on a track, the song page, or the whole project. One tool per file in `extensions/`, `.tsx` with a view. Import everything from `./sdk`; `sdk.ts` lists every name by the module of `sdk/` that documents it. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`, which the runtime writes.

## Check your work, after every change

Run 1 and 2 in `extensions/`, the rest with the app's program `sound-tools-next` (`$SOUND_TOOLS_RUNTIME` when set) on the project folder.

1. `bun check -p .`: types, token names, props, units, a press without a key or a label.
2. `bun test`: your `*.test.tsx`, one test for each thing the tool must do: `render(tool, self)`, then `find`, `press`, `act` and `apply`, as `song.test.tsx` does.
3. `sound-tools-next . --snap out.png --play 2`: a picture after two seconds of playing. Look at it: what lights, what is cut off or overlaps.
4. `--tree out.json`: every element with its text as shown, and `controls` (key, label, role, value or `checked`, actions), `playing`, `playhead`, `meters` in dB and `problems`. Check each control you made is there and says its state.
5. `--act steps.json` runs actions before the picture, on the project itself, as a click: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "fader-t00", "action": "set", "value": -6 }, { "action": "undo" }]`. `--script steps.json` points by element key: `{"events": [{ "t": 0.5, "type": "tap", "key": "mute-t01" }, { "t": 1, "type": "drag", "key": "fader-t00", "dy": 40 }]}`. Snap or tree after either to see what changed.

Read `problems.txt` after each change: what does not load says why there. `--inspect` prints the records; `--analyze` and `--render` play the sound.

## A tool

```tsx
import { boolean, h, input, list, number, tool } from "./sdk";
import { Knob } from "./ui/knob";
import { Toggle } from "./ui/toggle";

export const chop = tool({
  name: "chop",
  title: "Chop",
  when: "You want a sound cut in a rhythm of sixteen steps",
  doc: "Lets the sound through on the steps that are on, `depth` how far it cuts the others.",
  state: { steps: list(boolean({ default: true }), { max: 16 }), depth: number({ min: 0, max: 1, default: 1 }) },
  sound: ({ depth }) => input.times(depth.negate().plus(1)),
  view: (self, { update }) => (
    <col gap={2}>
      <row gap={1}>
        {Array.from({ length: 16 }, (_, i) => {
          const on = self.state.steps[i] ?? true;
          const flip = () => update(`Step ${i + 1}`, (draft) => void (draft.state.steps = Array.from({ length: 16 }, (_, j) => (j === i ? !on : (self.state.steps[j] ?? true)))));
          return <Toggle id={`step-${i}`} label={`Step ${i + 1}`} letter={`${i + 1}`} on={on} color="green" onToggle={flip} />;
        })}
      </row>
      <Knob id="depth" label="Depth" value={self.state.depth} min={0} max={1} onChange={(depth) => update("Depth", (draft) => void (draft.state.depth = depth))} />
    </col>
  ),
});
```

- `when` and `doc` are what the next agent reads. A number field is a `Signal` in `sound`, never a number.
- To put a device on a track, add its record to `state/song/<track>/chain/<name>.json` as `{"tool": "chop", "state": {}}`, and its name to `order` in the chain's `instance.json`.
- A tool without `view` gets a card with a control per field, the first four shown.

## Views

`view: (self, ctx) => …` is a plain function of the records, `self.state` and `self.children`, run again after every change and action: no hooks, no signals. `sdk/view.ts` documents every element and prop. Never name a variable `h`: JSX calls `h`. A part of a view in its own function types its records with `Drawn<ValuesOf<typeof fields>, typeof children>`.

- `ctx` is what `tick` gets: `update`, `send`, `play`, `clock` (`play`, `stop`, `jump`), `memory`. Use them in handlers.
- `memo(key, deps, () => …)` builds a part again only when a dep changes: `deps` lists all it shows. Build its keys from what is in `deps`, never an index.
- `memory` is never saved, such as the chosen track. Changed in `tick` it does not draw the view again: change it in a handler or an `update`.
- What does not fit is cut off: give a box `scroll="x"`, `"y"` or `"both"`, with `maxH` to grow until then.

## Look

Colours, text styles, spacing, radius and shadows are names from `extensions/theme.json` (`fill="surface"`, `gap={2}`, `style="label"`). `"green/25"` is a colour at 25%. Components are files in `extensions/ui/`: read them, change them, add your own. A value is the one bright line (`text`), green is sound, a toggle that is on is tinted and hollow when off.

## What moves every frame

The view never runs for what moves: a live value is a small expression the window works out every frame. Start from `live` (`playhead`, `elapsed`, `peak(path)`, `field(name)`, `sounding(pitch)`) and combine with `Live`'s methods in `sdk/values.ts`: arithmetic, comparisons, `decibels`, and over time `hold`, `fall`, `smooth`.

- A `text` shows one with `live` and `unit` (`"db"`, `"percent"`, `"position"`, `"time"`, …) or `digits`: `<text live={live.peak("t01/mix").hold(3).decibels()} unit="db" />`.
- A `box`'s or `shapes`'s `at`, `w`, `h` and `opacity` take one, as `ui/meter.tsx` does. A key that lights while it sounds: `opacity={live.sounding(60).max(0.3)}`.
- Code cannot read a live value: read the clock in `tick` or a handler.

## Changes and actions

- `update(label, (draft) => …)` changes records: write to `draft` as to the records. All the changes of one press, action, drag or paint are one step of undo. A change the records refuse fails the action.
- An element a pointer or an agent uses has a unique `key`, a `label` that says what it is ("Mute Track 2"), its handlers and its state (`checked`, `value`). Handlers: `onPress`, `onDragEnd`, `onEnter` (a paint: once for each element a held press enters, such as cells of a grid) and `actions` such as `toggle` or `set`.
- A `drag` moves the element in the window; `onDragEnd` hears where it stopped.
- `tick` runs about 30 times a second while the window is open, also in `--play`.
