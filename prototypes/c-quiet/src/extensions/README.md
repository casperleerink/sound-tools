# Writing an example extension (prototype C)

An extension is one folder `src/extensions/<type>/index.tsx` that exports `tool: ToolDefinition` (see `src/core/sdk.ts`). The core places each view in a `ToolFrame` card on the canvas and draws its ports and wires. Views never render their own header, title, close button or ports.

## Rules

- Use only the shared primitives from `@/ui` (`src/ui/*.tsx`): `Button`, `IconButton`, `Badge`, `Kbd`, `Tooltip`, `SegmentedControl`, `Tabs`, `Switch`, `Separator`, `Field`, `NumericInput`, `Slider`, `Knob`, `Meter`. Plus at most **one** custom SVG (or canvas) area per extension for the musical interface.
- The body of a card is neutral. The instance accent colours the header dot, the ports and the wires only. Do not use `var(--accent)` inside a view; sliders, knobs and switches fill with the text colour (`gray-950`) on their own. In SVG use `var(--color-gray-950)` for the active thing and `var(--color-alpha)` at 6 to 12% opacity for the rest.
- Style with Tailwind classes and design tokens only. Never hardcode hex colours. Text: `text-gray-950` (main), `text-gray-900` (labels), `text-gray-800`/`text-gray-700` (meta). Surfaces: `bg-alpha/5`, borders `border-alpha/10`.
- Spacing on a 4px grid inside controls, 8px between groups: `gap-2`, `gap-3`, `gap-4`. The card already pads the body by 16px. Labels `text-xs font-medium text-gray-900`. All numbers, times and values use the `tabular` utility.
- Show only what a composer needs while playing. Everything else goes in a second view (not open by default) or behind a disclosure. One readout per row at most; no captions or explanatory sentences.
- Every control has an accessible name (wrap in `Field`, or pass `label`). Keyboard: primitives already handle it.
- Interactivity is local `React.useState` with fake data. Drags in the custom area are welcome (pointer events + `setPointerCapture`). No audio, no timers unless purely visual.
- Multiple views of one instance must show the same data: put fake data in a module-level store and read it from every view.
- Card width is fixed by `ViewDef.width`; height follows content. Keep views compact: a main musical view around 150 to 250px of body height, a parameter view under 220px.
- Do not modify anything outside your extension folder. Do not touch `src/ui`, `src/core`, or the registry.
- Preview: `http://localhost:5176/#/ext/<type>` shows every view of one tool side by side. Screenshot with `agent-browser --session <type> set viewport 1000 700`, `open <url>`, `screenshot /tmp/<type>.png`, then look at the PNG.
- Verify with `bun run typecheck` from the project root before reporting.
