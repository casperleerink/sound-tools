# Writing a tool for this project

Write a tool in TypeScript, with no build: a device on a track, a change of the song page, or the whole project as one tool. One tool per file in `extensions/`, `.tsx` with a view. Import everything from `./sdk`; `sdk.ts` lists every name by the module of `sdk/` that documents it. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`, which the runtime writes. A save while the app is open loads the tools again.

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

- `name` is no built-in tool's. `when` and `doc` are what the next agent reads. A number field is a `Signal` in `sound`, never a number: turning a knob does not build the sound again.
- To put a device on a track, add its record to the track's chain, `state/song/<track>/chain/<name>.json` as `{"tool": "chop", "state": {}}`, and its name to `order` in the chain's `instance.json`. The chosen track's panel shows the chain in that order.
- A tool without `view` gets a card with a control per field.

## Views

`view: (self, ctx) => …` is a plain function of the records, `self.state` and `self.children`, run again after every change and action: no hooks, no signals. `sdk/view.ts` documents every element and prop. Never name a variable `h`: JSX calls `h`. A part of a view in its own function types its records with `Drawn<ValuesOf<typeof fields>, typeof children>`, never `typeof` its own tool.

- `memo(key, deps, () => …)` builds a part again only when a dep changes: `deps` lists all it shows. Build its keys from what is in `deps`, such as a track's name, never its index.
- `ctx.memory` is the tool's `memory`, which `tick` gets too: never saved, such as the chosen track. Changed in `tick` it does not draw the view again: change it in a handler or an `update`.

## Look

Colours, text styles, spacing, radius and shadows are names from `extensions/theme.json` (`fill="surface"`, `gap={2}`, `style="label"`); a wrong name is a type error. `"green/25"` is a colour at 25%. Components are files in `extensions/ui/`: read them, change them, add your own. Follow their rules: a value is the one bright line (`text`), green is sound, a toggle that is on is tinted and hollow when off.

## What moves every frame

The view never runs for what moves: a live value is a small expression the window works out every frame. Start from `live` (`playhead`, `elapsed`, `peak(path)`, `field(name)`) and combine with `Live`'s methods in `sdk/values.ts`: arithmetic, comparisons, `decibels`, and over time `hold`, `fall`, `smooth`.

- A `text` shows one with `live` and `unit` (`"db"`, `"percent"`, `"position"`, `"time"`, …) or `digits`: `<text live={live.peak("t01/mix").hold(3).decibels()} unit="db" />`.
- A `box`'s or `shapes`'s `at`, `w`, `h` and `opacity` take one, as `ui/meter.tsx` does.
- Code cannot read a live value: read the clock in `tick` or a handler.

## Changes and actions

- `ctx` is what `tick` gets: `update(label, (draft) => …)` changes records (write to `draft` as to the records), `send`, `play` and `clock` play. All the changes of one press, action or drag are one step of undo. A change the records refuse fails the action.
- An element a pointer or an agent uses has a unique `key`, a `label` that says what it is ("Mute Track 2"), its handlers (`onPress`, `onDragEnd`, `actions` such as `toggle` or `set`) and its state (`checked`, `value`).
- A `drag` moves the element in the window; your code hears only where it stopped.
- `tick` runs about 30 times a second while the window is open, also in `--play`. What it `send`s plays, and `clock.stop()` and the like move the piece.

## Check your work

In `extensions/`:

- `bun check -p .` checks types: token names, props, units, a press without a key or a label.
- `bun test` runs `*.test.tsx`: `render(tool, self)`, `find`, `press`, `act` and `apply`, as `song.test.tsx` shows.

With the app's program, `sound-tools-next` (`$SOUND_TOOLS_RUNTIME` when set), on the project folder (`.` from inside it):

- `--snap out.png`: a picture of the window. Look at it.
- `--tree out.json`: every element with its text as it shows, and `controls`: each control's key, label, role, value or `checked`, and actions. Also `playing`, `playhead`, `meters` (in dB) and `problems`.
- `--act steps.json`: runs actions before the picture, on the project itself, as a click would: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "fader-t00", "action": "set", "value": -6 }, { "key": "fader-t00", "action": "increment" }, { "action": "undo" }, { "action": "redo" }]`. A step that fails, or whose change is refused, fails the run with why.
- `--play 2`: plays two seconds from the start before the picture or the tree.
- `--script steps.json`: pointer and keys by element key, such as `{"events": [{ "t": 0.1, "type": "tap", "key": "mute-t01" }, { "t": 0.5, "type": "drag", "key": "fader-t00", "dy": 40 }]}`.
- `--inspect` prints the records; `--analyze` and `--render` play the sound.

Read `problems.txt` after each change: what does not load says why there.
