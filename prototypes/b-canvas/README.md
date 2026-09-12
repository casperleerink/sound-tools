# Sound Tools prototype B: canvas workspace

Non-functional design prototype of the Sound Tools window. Tool instance views are cards on a pannable, zoomable canvas with visible ports and wires, an agent sidebar on the left, and a floating transport pill. React 19, TypeScript, Vite, Tailwind v4, fake data only.

## Run

```
bun install
bun dev            # http://localhost:5174
bun run typecheck
bun run build
```

## Screens

- `/` the window. `?state=building` and `?state=failed` switch the agent and build story; the "prototype" row at the bottom of the sidebar does the same.
- `#/gallery` every primitive with all variants, sizes and states (hover, focus, active, disabled forced with `data-hover` / `data-focus`).
- `#/ext/<type>` one extension with all its views, for authoring. Types: `polyrhythm`, `curve`, `sampler`, `additive`, `mixer`.

Screenshots at 1440x900 are in `screenshots/`: `default.png`, `building.png`, `failed-build.png`, `primitives-gallery.png`.

## What is here

- `src/core/` the core: `sdk.ts` (the contract an extension implements), `Workspace.tsx`, `Canvas.tsx` (cards, pan, zoom, wires), `AgentSidebar.tsx`, `TransportPill.tsx`, `ProjectCluster.tsx`, `project.ts` (fake instances, card placement, connections), `conversation.ts` (one agent story, three endings).
- `src/ui/` the UI SDK primitives, adapted from Hooman Studio and recoloured to Catppuccin Mocha: button, badge, kbd, tooltip, segmented control, tabs, switch, separator, field, numeric input, slider, knob, meter, and `tool-frame` (the card with header and ports).
- `src/extensions/` five example tools, each using only the primitives plus at most one custom SVG area: Polyrhythm (stretch a pattern by dragging its right edge; a second Voices view edits the same data), Curve (draw a modulation shape; Points view), Sampler (waveform with region handles; Voice view), Additive (eight partial sliders; Voice view), Mixer (strips; Sends view). `src/extensions/README.md` is the authoring guide.

## Design decisions

- Surfaces: crust is the window and sidebar, mantle is the canvas with a dot grid, base is every card and pill. One accent per tool instance (peach, pink, yellow, blue, teal); lavender is the agent, green is play, peach warns, red errors. Solid fills use dark ink text.
- Signal flows left to right: event and modulation sources on the left, instruments in the middle, the mix on the right. Ports sit on the card edges (disc = audio, ring = events, diamond = modulation); wires take the source instance's colour and differ by dash style, so kind and origin can be read without a legend. The legend sits next to the zoom control anyway.
- Only one card per instance carries ports. A second view of the same instance shows a "2" marker in its header and a dotted link to its sibling; Polyrhythm and Mixer are both open twice.
- The canvas fits all cards to the window on load (90% at 1440x900, 70% at 1200x800). Zoom buttons and "fit" work; headers drag cards; the background pans.
- Build state lives in two places on purpose: as tool-call rows in the conversation (read, edit record, edit source, build, reload, with "no build" on project edits) and as a chip in the transport pill, since a reload stops playback.
- No fixed top bar. A 28px drag strip runs across the top; the sidebar header leaves room for macOS traffic lights.
- Accessibility: every control has a name, focus rings are visible on all primitives, ports and stretch handles are keyboard reachable.

## What to look at

Whether cards on a canvas feel like instruments rather than windows; whether five accents plus lavender and green is still restrained; whether the headers stay readable at narrow card widths; whether wires are enough to understand routing, or a routing view is needed.
