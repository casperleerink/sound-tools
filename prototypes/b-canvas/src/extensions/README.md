# Writing an example extension (prototype B)

An extension is one folder `src/extensions/<type>/index.tsx` that exports `tool: ToolDefinition` (see `src/core/sdk.ts`). The core places each view in a `ToolFrame` card on a canvas and draws its ports and connections. Views never render their own header, title, close button or ports.

## Rules

- Use only the shared primitives from `@/ui` (`src/ui/*.tsx`): `Button`, `IconButton`, `Badge`, `Kbd`, `Tooltip`, `SegmentedControl`, `Tabs`, `Switch`, `Separator`, `Field`, `NumericInput`, `Slider`, `Knob`, `Meter`. Plus at most **one** custom SVG (or canvas) area per extension for the musical interface.
- Style with Tailwind classes and design tokens only. Never hardcode hex colours. Text: `text-gray-950` (main), `text-gray-900` (labels), `text-gray-800`/`text-gray-700` (meta). Surfaces: `bg-alpha/5`, borders `border-alpha/10`, inset areas `bg-gray-100` or `bg-gray-50`. The instance accent is `var(--accent)` and is already set on the card: use `bg-(--accent)`, `text-(--accent)`, `border-(--accent)/40`, `bg-(--accent)/10`, and in SVG `stroke="var(--accent)"`, `fill="var(--accent)"`. Muted grid lines in SVG: `stroke="var(--color-gray-400)"`. Solid accent fills need dark text: `text-ink`.
- Spacing on a 4px grid: `gap-2`, `gap-3`, `p-3`. Labels `text-xs font-medium text-gray-900`. All numbers, times and values use the `tabular` utility. Radii: `rounded-md` for small things, `rounded-lg` for controls, inset areas `rounded-lg`.
- Every control has an accessible name (wrap in `Field`, or pass `label`). Keyboard: primitives already handle it.
- Interactivity is local `React.useState` with fake data. Drags in the custom area are welcome (use pointer events + `setPointerCapture`). No audio, no timers unless purely visual.
- Fake data must tell a musical story: real-looking names, values, units.
- Multiple views of one instance must show the same data: put fake data in a module-level constant and read it from every view.
- Card width is fixed by `ViewDef.width`; height follows content. Keep views compact: a main musical view around 200 to 300px of body height, a parameter view under 220px.
- Do not modify anything outside your extension folder. Do not touch `src/ui`, `src/core`, or the registry. If a primitive is missing something, work around it and mention it in your report.
- Preview: the dev server is running at `http://localhost:5174/#/ext/<type>`. Screenshot with an isolated browser session: `agent-browser --session <type> set viewport 1000 700`, `agent-browser --session <type> open http://localhost:5174/#/ext/<type>`, `agent-browser --session <type> screenshot /tmp/<type>.png`, then look at the PNG. Close with `agent-browser --session <type> close`.
- Verify with `bun run typecheck` from the project root before reporting.
