# Writing a tool

A tool is one `.tsx` file that exports `view(state)`: a function from the records to what the
window shows. It runs again after every change, so write it as plain code: no hooks, no
signals. Elements and types come from `./sdk`; JSX needs no import.

```tsx
import { change, memo } from "./sdk";
import { Toggle } from "./ui/toggle";

type State = { tracks: { name: string; mute: boolean }[] };
const edit = change<State>;

export const view = (s: State) => (
  <col pad={4} gap={2} fill="surface" radius="md">
    {s.tracks.map((track, i) => (
      <row gap={2} align="center">
        <text style="sm">{track.name}</text>
        <Toggle id={`mute-${i}`} label="M" on={track.mute} color="orange"
          onPress={() => edit((d) => void (d.tracks[i]!.mute = !d.tracks[i]!.mute))} />
      </row>
    ))}
  </col>
);
```

## Elements

- `row`, `col`, `box`: layout (`w`, `h` in points or `"full"`, `grow`, `pad`, `gap`, `align`,
  `justify`, `at` to place it, `inset` to pin it, `clip`, `scroll`) and look (`fill`, `border`,
  `radius`, `shadow`, `opacity`). `sdk.ts` lists every prop.
- `text`: `style`, `color`, `truncate`.
- `shapes`: line segments. With `units` they are in your own units (ticks, pitches) and stretch
  to `w` × `h`, so a zoom doesn't build them again.

## Design

Colours, text styles, spacing, radius and shadows are names from `theme.json`
(`fill="surface"`, `gap={2}`, `radius="md"`); a wrong name is a type error. `"text/50"` is a
colour at 50%; a record's own colour may be `#rrggbb`. Change the look of every tool in
`theme.json`. Components are files in `ui/`: read them, change them, add your own.

## Changes and input

- `change<State>((draft) => …)` asks for a change of records: write to `draft` as to the state.
  It shows at once and is saved by the app. Read state from `draft` in a handler, not from the
  view.
- `onPress` and `onDragEnd` need a `key`, unique in the view. A `drag` moves the element in the
  window while the pointer moves; `onDragEnd` hears how far.
- What moves every frame is bound by name, never set from code: `x={{ live: "playhead", scale }}`,
  `level="meter-3"`, `<text live="time">`.

## Big views

Wrap a part in `memo(key, deps, () => …)` and it is built and sent again only when a dep
changes (as React's `useMemo`): a track's lane on its clips and the zoom.

## Check your work

- `tsc -p .` checks types; `bun test` runs `*.test.tsx`.
- In a test, `render(view, state)`, then `find(key)` gives an element and `press(key)` the
  changes a press asks for (`song.test.tsx`).
- In the window: `--snap out.png`, `--tree out.json`, `--script steps.json` (clicks and drags by
  key).
