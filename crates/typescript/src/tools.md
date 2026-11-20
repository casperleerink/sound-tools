# Writing a tool for this project

A tool is TypeScript: a device on a track, the song page, or the whole project. One tool per file in `extensions/`. Import from `./sdk`, which lists every name by its module of `sdk/`. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`.

## Check your work, after every change

Run 1 and 2 in `extensions/`, the rest with `sound-tools` on the project folder.

1. `bun check -p .`: types, token names and props.
2. `bun test`: one test for each thing the tool must do, as `song.test.tsx` does: `render(tool, self, { held, facts, answers })`, then `find`, `press(key, { x, right })`, `act(keyOrLabel, "drag", { dx, alt, over })`, `act(k, "drop", { audio })`, `act(k, "set", 6)`, `act("song.record")` (an action of the tool), and `apply`. `act` drags within the drag's bounds and on its grid, as the window does.
3. `sound-tools . --snap out.png --play 2`: a picture after two seconds of playing.
4. `--tree out.json`: every element, and `controls` (key, label, role, value or `checked`, actions), `meters` and `problems`.
5. `--act steps.json` runs actions first, on the project itself: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "fader-t00", "action": "set", "value": -6 }, { "action": "undo" }]`. `--script steps.json` points by key: `{"events": [{ "t": 2, "type": "drag", "key": "fader-t00", "dy": 40 }]}`.

`problems.txt` says why what does not load. `--inspect` prints the records; `--analyze` and `--render` play the sound.

## A tool

```tsx
import { h, input, number, tool } from "./sdk";
import { binder } from "./ui/field";
import { Knob } from "./ui/knob";
import { DeviceCard } from "./ui/blocks/device-card";

const fields = { depth: number({ min: 0, max: 1, default: 1 }) };

export const quieter = tool({
  name: "quieter",
  title: "Quieter",
  when: "You want a sound quieter by a set amount",
  doc: "Lets through `depth` of the sound.",
  state: fields,
  sound: ({ depth }) => input.times(depth),
  view: (self, ctx) => <DeviceCard id="quieter" title="Quieter" cells={[<Knob {...binder(self, ctx, fields)("depth")} />]} />,
});
```

- The next agent reads `when` and `doc`. In `sound` a number field is a `Signal`.
- To put a device on a track, add `state/song/<track>/chain/<name>.json` as `{"tool": "quieter", "state": {}}`, and its name to `order` in the chain's `instance.json`.
- A device without a view gets a card: `ui/blocks/cards/<tool>.tsx` exporting `card: Card<"eq">` for a built-in tool, else `card: Card` of `ui/blocks/generic-card.tsx`.

## Views

`view: (self, ctx) => …` is a plain function of `self.state`, `self.children` and `self.facts`, run again after every change. Never name a variable `h`. A part of a view types its records with `Drawn<ValuesOf<typeof fields>, typeof children>`.

- `ctx` is what `tick` gets: `update`, `send`, `play`, `clock`, `memory`, `held`, `catalog` (devices to put in a child with `addOffer`), `shortcut(action)` (its key now).
- `self.facts`, and each child's: what the window knows that no record holds, such as `title`, `fields`, a region's `seconds`.
- `memo(key, deps, () => …)` builds a part again only when one of `deps` changes.
- `memory` is never saved: change it in a handler to draw again.

## Look: theme, primitives, components, blocks

Use the top layer that fits; each uses only those under it.

1. Theme, `extensions/theme.json`: props name its tokens (`fill="surface"`, `gap={2}`, `"sound/25"` at 25%). `sound` (green) is sound that moves; what is chosen takes `selected`.
2. Primitives, each prop in `sdk/view.ts`: `box`, `row`, `col`, `text`, `shapes`, `field`, `icon` (`ui/icons/<name>.svg`), `child`, `live`; `hoverGroup` and `reveal` (the quiet rule), `overlay`, `group` (one Tab stop).
3. Components, `extensions/ui/`: Button, Toggle, Select, Menu, Popover, Tooltip, Knob, Slider, Fader, Meter, Pad, TextField, Cell and more; `value.ts` steps, clamps and reads typed values.
4. Blocks, `extensions/ui/blocks/`: DeviceCard, cards, Panel, StepGrid, Keyboard, PaintGrid and more.

Change them, add yours; `--main gallery --snap g.png --size 1470x1500` shows them.

- A device is a `DeviceCard`: a display, then cells, two to a column, growing wider.
- A card's controls are `sm`. Use the app's transport: add no Play button.
- Every control takes the keys: `actions` (`increment`, `decrement`, `set`, `reset`); many go in a `group`.

## Planes: time, pitch, your own units

A box with `plane={{ x: 0.03, y: 12 }}` (points per unit, or a `[from, to]` range it shows) has units of its own; the window scrolls and zooms it. Inside it, at any depth, `place={[start, pitch, length, 1]}` puts a box in those units; `place={[x, y]}` is a mark, its `w`/`h` points centred there. Everything else stays points. Pointers inside it are in its units.

## What moves every frame

A live value is an expression the window works out every frame, without the view. Start from `live` (`playhead`, `playing`, `elapsed`, `bpm`, `field(name)`, `feed(name, path)`, `drag(id)`, `plane(key)`) and combine with `Live`'s methods: arithmetic, comparisons, `decibels`, `hold`, `fall`, `smooth`.

- `<text live={live.feed("peak", "t01/mix").hold(3).decibels()} unit="db" />`; `opacity={live.feed("sounding.60")}`.
- A feed that is a list draws as `points` or `rects` of a `shapes`.
- `at`, `place`, `w`, `h`, `opacity` and `trim` take one, as `ui/meter.tsx` does. Code cannot read one.
- `binder(self, ctx, fields)` of `ui/field.ts` wires a number's control, also what a lane plays.

## Changes and actions

- `update(label, (draft) => …)` changes records. All the changes of one press, drag or paint are one step of undo. An update of a number a lane plays (`held`) is refused.
- An element a pointer or an agent uses has a unique `key`, a `label` ("Mute Track 2") and its state (`checked`, `value`). Handlers: `onPress(p)` and `onDragEnd(p)` hear a `Pointer` (`x`, `y`, `dx`, `dy`, `clicks`, `right`, `shift`, `alt`, `cmd`, `over`: the key under it at the end), `onDrop(d)` hears files (`d.audio`, `d.parts`), `onEnter` paints, a `field`'s `onCommit(text)` (`null` on Escape), `actions`.
- `drag={{ x: [-start, 9999], snap: [240, 1] }}` moves a clip along time on sixteenths; `live.drag(key)` follows it and `onDragEnd` keeps it with `update`. `plays: { gain_db: live.drag("gain").dy.negate().plus(gain) }` sounds a value while the drag is held.
- `<col overlay={{ open: "menu" }} role="menu">` in a box opens at a right press on it: its items are boxes with `onPress`.
