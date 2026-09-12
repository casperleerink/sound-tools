# Sound Tools prototype A: docked panes

High-fidelity, non-functional prototype of the Sound Tools window. React 19, TypeScript, Vite, Tailwind v4. No audio, no real agent, no persistence. Direction: docked, dense, quiet. Think Zed or a well-made DAW, not Max/MSP.

## Run

```
bun install
bun dev            # http://localhost:5181
bun run build
bun run typecheck
```

## Screens

- `/` the window. The `dev` toggle in the top-right of the drag strip switches the build state: idle, building, failed. Same via `?state=building` or `?state=failed`.
- `#/gallery` the primitives gallery: every primitive with its variants, sizes and states.
- `screenshots/`: `default.png`, `building.png`, `failed.png`, `gallery.png` at 1440x900.

## What is where

- `src/globals.css` tokens. Hooman Studio scale recoloured to Catppuccin Mocha. crust is the window background, mantle the bars and rails, base the tool view surface, surface0 for popups and tooltips.
- `src/ui/` primitives: Button, IconButton, Badge, Dot, Kbd, Tooltip, Separator, SegmentedControl, Tabs, Switch, Select, LabelledControl, ControlGroup, NumericInput, Slider, Knob, Meter, ToolView, Panel.
- `src/sdk/` what an extension imports: tool and view types, `useInstanceState` (shared per-instance state, so two views of one instance stay in sync), the view context the core fills in.
- `src/app/` the shell: drag strip, left project rail, pane tree with tab strips, agent sidebar, transport bar.
- `src/agent/` the agent sidebar: conversation, tool call rows, build output, composer with model picker.
- `src/extensions/` five example extensions built only with the primitives plus at most one custom SVG each:
  - Tone (instrument, teal): frequency, level, envelope, waveform.
  - Tremolo (effect, pink): rate, depth, shape, mix, sync.
  - Rhythm Loops (sequencer, yellow): stretchable pattern bars, two views (Patterns and Voices).
  - Ratio Lattice (instrument, blue): a 7-by-5 just-intonation ratio lattice you click to build a scale, two views (Lattice and Scale).
  - Mixer (utility, flamingo): a channel strip per input, meters, mute and solo, output routing.
- `src/data/` fake project: instances, connections, the conversation for each build state.

## Design decisions

- **Panes with tab strips.** Every tab is one view of one tool instance, with the tool's accent dot. Drag tabs between panes, split a pane right or down, add a view with `+`. Loops is open twice (Patterns on the left, Voices on the right); both carry a `2 views` badge and share state, so a change in one shows in the other.
- **Connections live in the view header.** A slim row under the title lists what feeds an instance and what it feeds. No patch cables; the agent makes connections, the composer only needs to find them.
- **Build state is visible in three places and calm everywhere.** The pill in the bottom bar, a badge on the extension in the left rail, and a `rebuilding` or `previous build` badge on every view of the extension being rebuilt. A failed build reads as "still running the previous build", not as an error dialog.
- **No top bar.** A 28px drag strip keeps the macOS traffic lights clear. Transport, device, project name, undo/redo and build status share one 36px bottom bar.
- **One accent per tool type.** Lavender is reserved for the agent and focus rings, green for transport, peach and red for warnings and errors. Tools pick from the rest so instances can be told apart at a glance.
- **Numbers are tabular everywhere**, position readouts are monospace, paths and build output are monospace.
- **Accessibility is built into the primitives.** Every control has a label, every button is keyboard reachable, and one lavender focus ring style is used throughout. Knobs, sliders and number boxes take arrow keys, Home, End and Shift for fine steps.
- On windows narrower than 1300px the project rail starts collapsed and the sidebar narrower, so 1200x800 still fits.
