# Writing a tool for this project

A tool is TypeScript: a device, a page, or the song. One tool per file in `extensions/`; import from `./sdk`. Do not edit `sdk.ts`, `sdk/` or `tsconfig.json`.

## Check your work, after every change

In `extensions/`:

1. `bun check -p .`: types, tokens, props, paths.
2. `bun test`: a test per thing the tool must do (see `song.test.tsx`): `render(tool, self, { held, facts, answers })`, then `find(keyOrLabel)`, `press(key, { x, right })`, `act(k, "drag", { dx, alt, over })`, `act(k, "drop", { audio })`, `act(k, "set", 6)`, `act("song.record")` (a tool action). Each returns the changes; `act` drags within bounds and on the snap, as the window does.

With `sound-tools` on the project folder:

3. `--snap out.png` draws it (`--play 2` plays first). Look at it.
4. `--tree out.json`: every control (key, label, role, value or `checked`, actions), `planes`, `feeds`, `problems`.
5. `--act steps.json` runs actions: `[{ "label": "Mute Track 2", "action": "toggle" }, { "key": "song", "action": "view", "value": { "x": [0, 3840] } }, { "action": "undo" }]`. `--script steps.json` points by key: `{"events": [{ "t": 2, "type": "drag", "key": "clip-1", "dx": 40 }]}`.

`problems.txt` says what does not load.

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
  view: (self, ctx) => <DeviceCard id="quieter" title="Quieter" cells={[<Knob {...binder(self, ctx, fields).number("depth")} />]} />,
});
```

- Sound: `sdk/sound.ts` and fields: `sdk/fields.ts` are the reference; read their doc comments. In `sound` a field is a `Signal`, which code cannot read: choose with comparisons and `mix`, never `?:`.
- Records live in `state/`: a page's own is `state/<name>.json`, `{"tool": "<name>", "state": {}}`; a folder only for a parent, its record `instance.json`. A device on a track: `state/song/<track>/chain/<name>.json`, its name in `order` of the chain's `instance.json`.
- A device without a view gets a card: for a built-in tool, `ui/blocks/cards/<tool>.tsx` exporting `card: Card<"eq">` (`Card<"eq", { open: boolean }>` types its memory); else `ui/blocks/generic-card.tsx`. Test one with `render(card, { tool: "eq", state: { … } })`: its facts and the defaults the state leaves out are the tool's.

## Views

`view: (self, ctx) => …` is a plain function of `self.state`, `self.children` and `self.facts`, run after every change. Never name a variable `h`.

- `ctx` is what `tick` gets: `update`, `send`, `play`, `clock`, `memory` (never saved; a change draws again), `held`, `catalog`, `shortcut(action)` (its key, drawn), `show(plane, { x: [from, to] })`.
- `self.facts`: what no record holds (`title`, `fields`, a region's `seconds`); `sdk/feeds.ts` names each tool's facts and feeds.
- `memo(key, deps, () => …)` builds a part again only when a dep changes; deps compare as data. Its handlers always see the newest run, so read `self` and anything the view worked out. Only after an `await` or your own `update` is `self` old: read `ctx.state`.

## Look: theme, primitives, components, blocks

Use the top layer that fits.

1. Theme, `extensions/theme.json`: props name its tokens (`fill="surface"`, `pad={[2, 1]}`, `"sound/25"` at 25%). `sound` (green) is sound that moves.
2. Primitives, each prop in `sdk/view.ts`: `box`, `row`, `col`, `text`, `shapes`, `field`, `icon` (`ui/icons/<name>.svg`), `child`; `hoverGroup` and `reveal` (the quiet rule), `overlay`, `group`.
3. Components, `extensions/ui/`: Button, Toggle, Select, Menu, Popover, Tooltip, Knob (`said`, `disabled`), Slider, Fader, Meter, Pad, TextField, Cell, Handle, Display.
4. Blocks, `extensions/ui/blocks/`: DeviceCard, cards, Panel, StepGrid, Keyboard, PaintGrid; `time.tsx`: Grid, Playhead and Clip for any timeline.

`--main gallery --snap g.png` shows them. The app has the transport: add no Play button.

## Planes: time, pitch, your own units

A box with `plane={{ x: 0.025, y: 64, zoom: [0.0025, 0.2], follow: live.playhead }}` has units of its own: `x` and `zoom` are points per unit (or `x: [from, to]`, the range it shows); the window scrolls and zooms it and runs no view. Inside it, at any depth:

- `place={[start, row, length, 1]}` puts a box in units; `inset={[0, 4, 0, 4]}` keeps it 4 points in from those edges at any zoom.
- `place={[x, y]}` is a mark: its `w` and `h` are points, centred there.
- Its children are laid out in points: an edge 6 points wide is a child `inset={[null, 0, 0, 0]} w={6}`.
- `live.plane(key)` gives `left`, `right`, `top`, `bottom` in units and `zoom` in points per unit: `place={[view.left, row]}` stays at the left edge.

Pointers are in units.

## What moves every frame

The window works a `Live` value out each frame. Start from `live` (`playhead`, `playing`, `elapsed`, `bpm`, `field(path)`, `feed(name, path)`, `drag(id)`, `plane(key)`) and combine with its methods.

- `<text live={live.feed("peak", "t01/mix").decibels()} unit="db" />`.
- A list feed is the `points` or `rects` of a `shapes`.
- Paths: `cutoff_hz`, `bands[0].gain_db`, `pads.36.pan`, `osc_1.position`, or inside a child, `t01/mix.gain_db`. `binder(self, ctx).number(path)` of `ui/field.ts` wires a control: lane, sound while dragging, one undo step.

## Changes and actions

- `update(label, (draft) => …)` changes records; one press, drag or paint is one undo step. An update of a number a lane plays (`held`) is refused.
- What a pointer or agent uses has a unique `key`, a `label` ("Mute Track 2") and its `value` (a switch's is `true`/`false`). Handlers: `onPress(p)` and `onDragEnd(p)` hear a `Pointer` (`x`, `y`, `dx`, `dy` in units, `clicks`: 0 for Enter, `right`, `shift`, `alt`, `cmd`, `over`), `onDrop(d)` files, `onEnter` paints, a `field`'s `onCommit(text)` (`null` on Escape), `actions` (`increment` and `decrement` are the arrow keys).
- `drag={{ x: [-start, null], snap: [240, 1] }}` moves on sixteenths, never before 0; `live.drag(key)` follows it, `onDragEnd` keeps it. `plays: { gain_db: live.drag("gain").dy.negate().plus(gain) }` sounds a value while held.
- `<col overlay={{ open: "menu" }} role="menu">` opens on a right press of its parent; `MenuList` of `ui/menu.tsx` draws one.
