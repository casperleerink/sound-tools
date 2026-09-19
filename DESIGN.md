# Design

Design decisions for the Sound Tools UI. Settled from the three web prototypes reviewed on September 10, 2026 (removed September 14; two screenshots remain in `docs/reference/`). The real UI SDK is built in GPUI in `crates/ui`; the gallery in `crates/gallery` shows every component and variant.

## Source design system

The UI takes its language from Hooman Studio (`~/hooman/hooman-studio/packages/ui`): tokens in `src/globals.css`, components in `src/components/`. Composite pieces to reuse: the chat composer (`packages/dashboard/src/messages/message-composer-form.tsx`, `chat/chat-pane.tsx`) and the sidebar (`packages/dashboard/src/tasks/sidebar/`).

- Font: InterDisplay with `ss03` and `cv01`. Tabular numbers (`tnum`, `lnum`) for every number, meter and time. Monospace stack for code, paths and build output.
- Sizes: controls 24, 28, 32, 40 px tall. Radii 6, 8, 10 px. Body 14 px, labels medium weight, meta 12 px. Agent conversation text 15 or 16 px with 1.5 line height.
- Fills use the alpha scale: `alpha/5` subtle surface, `alpha/10` hover and borders. Disabled is 40% opacity. Inactive tabs 40% opacity, 100% on hover or active.
- Variants: primary (solid text colour on background), subtle, outline, ghost, plus colour variants each with solid, subtle, outline and ghost.
- Motion: 100 ms colour transitions, ease `cubic-bezier(0.08, 0.82, 0.17, 1)`, a slow 3.5 s pulse to 70% opacity for "agent is working".
- Shadows: dropdown `0 4px 24px -8px rgba(0,0,0,0.2)`, card `0 8px 16px -8px rgba(0,0,0,0.1)`.
- Spacing: 8 px grid for layout, 4 px inside controls. Panel and card margins at least 24 px. Card padding at least 16 px.

## Colour: Catppuccin Mocha over the Hooman scale

Dark only for now. Hooman's dark theme uses `gray-50` as the darkest background and `gray-950` as text; keep that meaning.

| Token | Mocha | Hex |
| --- | --- | --- |
| gray-50 | crust | `#11111b` |
| gray-100 | mantle | `#181825` |
| gray-200 | base | `#1e1e2e` |
| gray-300 | surface0 | `#313244` |
| gray-400 | surface1 | `#45475a` |
| gray-500 | surface2 | `#585b70` |
| gray-600 | overlay0 | `#6c7086` |
| gray-700 | overlay1 | `#7f849c` |
| gray-800 | overlay2 | `#9399b2` |
| gray-900 | subtext0 | `#a6adc8` |
| gray-950 | text | `#cdd6f4` |
| alpha | white | `#ffffff` |
| blue | blue | `#89b4fa` |
| sapphire | sapphire | `#74c7ec` |
| sky | sky | `#89dceb` |
| teal | teal | `#94e2d5` |
| green | green | `#a6e3a1` |
| yellow | yellow | `#f9e2af` |
| peach | peach | `#fab387` |
| red | red | `#f38ba8` |
| maroon | maroon | `#eba0ac` |
| mauve | mauve | `#cba6f7` |
| pink | pink | `#f5c2e7` |
| lavender | lavender | `#b4befe` |
| rosewater | rosewater | `#f5e0dc` |
| flamingo | flamingo | `#f2cdcd` |

Solid colour buttons need dark text (`#1e1e2e`). Accents are sparse: lavender for the agent, green for play state, peach for warnings, red for errors. Each track or tool instance gets one accent, used on dots, ports and wires only, never on fills.

## Quiet rule

Every element must earn its keep. Reference feel: Hooman Studio and Hive. Lots of air, one accent, calm type. Audio tools are usually crowded; we are not copying that.

- Agent sidebar: a turn is the composer's message and the agent's result text. While working, one line such as `Building Polyrhythm` with a slow pulse. After, a muted `Worked for 12 s` that expands on click to the history. A failed build is `Build failed` in red plus one short sentence. No tool-call rows, progress bars, timestamps per message, or explanatory prose about builds and playback.
- Composer: input, model name, send. Placeholder inside the input is the only hint.
- Transport: floating pill, bottom centre. Play/pause, stop, position, duration if the project has one, a hairline seek strip. Build status and device selection are not in it; at most a small dot when a reload is pending.
- Chrome: project name top-left as a quiet menu holding add, undo/redo, output device and project folder. No legends, no zoom controls, no grid.
- Cards and panels: 16 px padding, no meta chips in headers, port labels on hover only, secondary parameters behind a disclosure or a second view.
- Accessible: visible focus rings, labelled controls, full keyboard reach. This is a product requirement.

## GPUI notes from the first port, September 14, 2026

The UI SDK lives in `crates/ui`; the gallery in `crates/gallery` shows every component (`GALLERY_SECTION=foundation|inputs|overlays|composed cargo run -p gallery`; `cargo test -p gallery --test snapshots` renders PNGs without opening a window). GPUI 0.2.2 limits that shaped the components. GPUI is now pinned to Zed v1.20.2, where some of these no longer apply, as noted:

- No CSS transitions. Hover and active states swap instantly. Only the switch thumb and the working indicator animate, through `with_animation`.
- No focus-visible. Focus rings show on mouse focus too. Stateless components take an optional `FocusHandle` to show a ring. The pinned version has `.focus_visible(..)`; the components do not use it yet.
- No built-in text widget. `text_input.rs` implements shaping, cursor, selection and IME itself. It is single-line; `.lines(n)` only makes the box taller. Real multi-line editing is future work.
- Key bindings are registered by the component on first use, scoped to a key context, since the app binds none globally yet.
- Draggable controls use drag events with a delta, so a plain click on a slider track does not jump the handle.
- SVG icons take an explicit colour; `Icon` reads the inherited text colour at render time. Colour buttons therefore tint rather than invert on hover.
- Overlays anchor to a zero-size box on the trigger edge and snap to the window with a margin. Side is explicit, not collision-aware. Click-outside uses `on_mouse_down_out` with an occluding surface.
- No arc primitive; the knob draws its value ring with dots.
- No accessibility tree in 0.2.2. The pinned version has AccessKit support (roles, labels, actions; see `crates/gpui/examples/a11y.rs` in Zed). The components do not use it yet.
