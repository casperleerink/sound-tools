# Writing a tool for this project

A tool is TypeScript: a device on a track, the song page, or the whole project. One tool per file in `extensions/`. Import from `./sdk`, which lists every name by its module of `sdk/`. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`.

## Check your work, after every change

Run 1 and 2 in `extensions/`, the rest with `sound-tools` on the project folder.

1. `bun check -p .`: types, token names and props.
2. `bun test`: one test for each thing the tool must do, as `song.test.tsx` does: `render(tool, self, { held, facts, answers })`, then `find`, `press(key, { x, right })`, `act(keyOrLabel, "drag", { dx, alt, over })`, `act(k, "drop", { audio })`, `act(k, "set", 6)`, `act("song.record")` (a tool action), and `apply`.
3. `sound-tools . --snap out.png --play 2`: a picture after two seconds playing.
4. `--tree out.json`: every element, `controls` (key, label, role, value or `checked`, actions), `meters` and `problems`.
5. `--act steps.json` runs actions first: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "song", "action": "view", "value": { "x": [0, 3840] } }, { "action": "undo" }]`. `--script steps.json` points by key: `{"events": [{ "t": 2, "type": "drag", "key": "fader-t00", "dy": 40 }]}`.

`problems.txt` says what does not load. `--inspect` prints the records; `--analyze` and `--render` play the sound.

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
- To put a device on a track, add `state/song/<track>/chain/<name>.json` as `{"tool": "quieter", "state": {}}`, its name in `order` of the chain's `instance.json`.
- A device without a view gets a card: `ui/blocks/cards/<tool>.tsx` exporting `card: Card<"eq">` for a built-in tool, else `ui/blocks/generic-card.tsx`.

## Views

`view: (self, ctx) => …` is a plain function of `self.state`, `self.children` and `self.facts`, run again after every change. Never name a variable `h`. A part of a view types its records as `Drawn<ValuesOf<typeof fields>, typeof children>`.

- `ctx` is what `tick` gets: `update`, `send`, `play`, `clock`, `memory`, `held`, `catalog` (devices for `addOffer`), `shortcut(action)` (its key now), `show(plane, { x: [from, to] })`.
- A handler reads records through `ctx.state` and `ctx.children`, always the newest; `self` is its run's.
- `self.facts`, and each child's: what no record holds, such as `title`, `fields`, a region's `seconds`.
- `memo(key, deps, () => …)` builds a part again only when a dep changes.
- `memory` is never saved; change it in a handler to draw again.

## Look: theme, primitives, components, blocks

Use the top layer that fits; each uses only those below.

1. Theme, `extensions/theme.json`: props name its tokens (`fill="surface"`, `gap={2}`, `"sound/25"` at 25%). `sound` (green) is sound that moves; the chosen take `selected`.
2. Primitives, each prop in `sdk/view.ts`: `box`, `row`, `col`, `text`, `shapes`, `field` (searches a menu), `icon` (`ui/icons/<name>.svg`), `child`, `live`; `hoverGroup` and `reveal` (the quiet rule), `overlay`, `group` (one Tab stop).
3. Components, `extensions/ui/`: Button, Toggle, Select, Menu, Popover, Tooltip, Knob, Slider, Fader, Meter, Pad, TextField, Cell; `value.ts` steps, clamps and reads typed values.
4. Blocks, `extensions/ui/blocks/`: DeviceCard, cards, Panel, StepGrid, Keyboard, PaintGrid.

Change or add them; `--main gallery --snap g.png --size 1470x1500` shows them.

- A device is a `DeviceCard`: a display, then `sm` cells, two to a column. The app has the transport: add no Play button.
- Every control takes keys (`actions`); many go in a `group`.

## Planes: time, pitch, your own units

A box with `plane={{ x: 0.03, y: 12 }}` (points per unit, or a `[from, to]` range it shows) has units of its own; the window scrolls and zooms it, and with `follow: live.playhead` keeps the playhead in view. Inside it, at any depth, `place={[start, pitch, length, 1]}` puts a box in those units; `place={[x, y]}` is a mark, its `w`/`h` points centred there; a box without `place` only groups them. Other sizes are points; pointers are in units.

## What moves every frame

A live value is worked out by the window every frame, without the view. Start from `live` (`playhead`, `playing`, `elapsed`, `bpm`, `field(name)`, `feed(name, path)`, `drag(id)`, `plane(key)`) and combine with `Live`'s methods: arithmetic, comparisons, `decibels`, `hold`, `fall`, `smooth`.

- `<text live={live.feed("peak", "t01/mix").hold(3).decibels()} unit="db" />`; `opacity={live.feed("sounding.60")}`.
- A feed that is a list draws as `points` or `rects` of a `shapes`.
- `at`, `place`, `w`, `h`, `opacity` and `trim` take one (`ui/meter.tsx`). Code cannot read one.
- `binder(self, ctx, fields)` of `ui/field.ts` wires a number's control: lanes, and the sound following its drag.

## Changes and actions

- `update(label, (draft) => …)` changes records; one press, drag or paint is one undo step. An update of a number a lane plays (`held`) is refused; with Write automation on while playing it writes the lane.
- An element a pointer or an agent uses has a unique `key`, a `label` ("Mute Track 2") and its state (`checked`, `value`). Handlers: `onPress(p)` and `onDragEnd(p)` hear a `Pointer` (`x`, `y`, `dx`, `dy`, `clicks`, `right`, `shift`, `alt`, `cmd`, `over`: the key under it at the end), `onDrop(d)` files, `onEnter` paints, a `field`'s `onCommit(text)` (`null` on Escape; `focus`: rename in place), `actions`.
- `drag={{ x: [-start, 9999], snap: [240, 1] }}` moves a clip on sixteenths; `live.drag(key)` follows it, `onDragEnd` keeps it with `update`. `plays: { gain_db: live.drag("gain").dy.negate().plus(gain) }` sounds a value while held; the `update` of `onDragEnd` keeps it as one step, none puts it back.
- `<col overlay={{ open: "menu" }} role="menu">` in a box opens at a right press on it; its items are boxes with `onPress`.
