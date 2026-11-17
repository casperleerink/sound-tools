# Spike: how tools of a project should be written

Question: if the window moves to Rune, how should an agent write a tool, from first principles?
Two variants of the same tool, both drawn by Rune, measured side by side:

- **ts/**: TypeScript in Bun, a separate process, as today. The view is a plain function of
  the state that runs again on every change; the tree goes to the window as JSON over a pipe.
- **luau/**: Luau scripts inside the app with `rune-luau` (watchdog, hot reload). No second
  process.

Same for both: Rune at the rev of `../rune-vs-gpui/rune-daw`, the song and scenarios of
`../rune-vs-gpui/model` (path dependency), `theme.json`, and the screen of
`../rune-vs-gpui/SPEC.md`, with a draggable fader and the time display.

## Rules for the SDK (what an agent writes against)

- Free layout from a few primitives: box/row/column, text, shapes (rects, lines) and the usual
  layout props. No card, page or `Controls` structure enforced.
- A design system like shadcn. Tokens in `theme.json`: colours, text styles (size, line height,
  tracking, weight), spacing, radius and shadow. Props name tokens (`fill: "surface"`,
  `radius: "md"`, `text: "sm"`). Components (Button, Toggle, Fader, Meter, Knob) are source
  files in the project's own `ui/` folder, written from the primitives, so an agent reads and
  changes them. Changing `theme.json` restyles everything with no code change.
- Records belong to the host (Rust here, as the core in the real app). The tool gets the state
  and asks for changes: a toggle or a fader's drag end is a change request the host applies, and
  the new state comes back to the tool.
- What moves every frame (playhead x, meters, the time) is bound by name to a value the host
  sets (`Live`), never through tool code each frame.
- Keep the SDK small. Every concept in it must earn its place. No reactivity runtime.
- Four layers, each built only from those below: (1) theme tokens; (2) primitives, behaviours
  and live values, the only part that is SDK; (3) components (button, slider, knob, list,
  menu); (4) blocks (device cards, track rows, the generic card, page layouts). 3 and 4 are
  project source. Growth goes there, never into SDK exports.

## The tool (`song`), in the project's folder

The DAW song page, one tool file plus the `ui/` components it uses. M/S/R toggles and the
fader send change requests. The fader moves while dragged without a round trip per move if the
variant can't afford one; its gain lands on release.

## What each variant's binary does

`<binary> --scenario idle|play|scroll|zoom|edit|click --tracks N --clips N --seconds S`,
release build, `rune` with `tooling` (so `--snap`, `--tree`, `--script` work). Prints one JSON
line on quit: `{"app","scenario","tracks","clips","seconds","frames","view_renders",
"click_ms_p50","click_ms_p95","bytes"}` (fields that don't apply are 0).

- `edit`: the host applies `model::apply_edit` each 100 ms and hands the tool the new state.
- `zoom`: the zoom is the tool's own state; the host tells the tool the zoom of
  `model::moment` each frame, so the view runs every frame: the worst case.
- `click`: every 250 ms the host presses `solo-<n>` (n cycles over tracks) through the same
  road a pointer press takes, and measures the time until the window shows the new tree.
- `bytes` (ts only): bytes sent each way over the pipe during the run.
- Element keys: `play`, `mute-<i>`, `solo-<i>`, `arm-<i>`, `fader-<i>`, `strip-<i>`.

## Verify

- `--snap` of play at 3.3 s looks like `../rune-vs-gpui/rune-daw`'s; changing `radius.sm` in
  `theme.json` to 0 changes the toggles' corners in a new snap.
- A test with no window: press `solo-1`, see the change request and the toggle lit.
- `measure.py` of `../rune-vs-gpui` runs it (APPS paths).
