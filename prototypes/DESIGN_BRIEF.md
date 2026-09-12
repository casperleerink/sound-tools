# Sound Tools design prototype brief

Shared brief for the two design prototypes in this folder. Written September 10, 2026.

## Goal

A high-fidelity, non-functional prototype of the Sound Tools window, built in web tech (React), so Casper can iterate on look and feel, lock in a design system, and understand the product by seeing it. Nothing has to work for real. Everything has to look finished.

Read `../CONCEPT.md` and `../ARCHITECTURE.md` first. Short version:

- One window. An **agent sidebar** and the project's **musical workspace** in the main area. No fixed top bar required.
- The **core** owns: the agent conversation, the transport (play, pause, stop, seek, project position), audio device selection, the project (folder of JSON records), build/reload of extensions, and a small **UI SDK** of shared primitives.
- **Extensions** provide tools: instruments, effects, editors, whole workflows. A tool instance has saved state, parameters, ports (audio, event, modulation), and one or more **views**. Composers mostly work in these views. The agent creates or edits extensions on request.
- Two speed budgets: project edits (turning a knob, moving a note) apply instantly. Extension code edits go through a **build** (about 2 seconds) and a **reload** that stops playback. A failed build keeps the old runtime running. The UI should make this visible and calm: "Building tremolo…", "Build failed, still running previous version", "Reloaded".
- The project folder is live: the agent edits JSON files, the UI follows. Undo/redo exists. Last write wins.
- Musical conventions are optional. Some tools use note names, some frequency ratios, some free timing. The design must not assume a DAW timeline.

## Where the time goes

Most of the effort goes into the **core**:

1. **Design tokens and primitives** (the UI SDK). This is what extensions and the agent will build with, so it has to be excellent and general.
2. **Agent sidebar**: conversation, streaming/working state, tool calls (reading a project record, editing a record, editing extension source, building, reloading), build status, the composer input with model/provider picker.
3. **Transport and project chrome**: play/pause/stop, position display, device output, project name, undo/redo, the build/reload status.
4. **General layout**: how tool instance views are arranged in the workspace, how you open a second view of an instance, how you close one, how connections between instances are shown or found.

Then, with the core done, add **example extensions** using only the primitives. Each version should ship at least four, ideally different from the other version's, covering these kinds:

- A small instrument with a few parameters (Tone: frequency, level; or a simple synth voice).
- An effect (Tremolo: rate, depth; or a delay/filter).
- A rhythmic/pattern tool where patterns can be stretched by dragging (from the concept doc).
- Something with a custom musical interface that is not a Western piano roll: a frequency ratio lattice, an additive rhythm grid, a drawing surface, a sample scrubber.
- Optional: a mixer, a modulation matrix, a simple arrangement/timeline extension, a MIDI input mapper.

Extension views must visibly share the same design language as the core, since the primitives are what make that happen. It is fine for one extension to also include a small custom drawn area (canvas/SVG) as that is allowed in the real product too.

## Design system: take Hooman Studio, recolour to Catppuccin Mocha

Source of truth for the design language: `/Users/casperleerink/hooman/hooman-studio/packages/ui/src/globals.css` and the components in `/Users/casperleerink/hooman/hooman-studio/packages/ui/src/components/` (read button, input, badge, chip, kbd, segmented-control, switch, tabs, tooltip, separator, card). Keep its feel:

- Font: **InterDisplay** with `"ss03", "cv01"` features. Copy the woff2 files from `/Users/casperleerink/hooman/hooman-studio/apps/web/public/fonts/inter-display/` into the prototype. Use a `tabular` utility (`tnum`, `lnum`) for all numbers, meters and times. Use a monospace stack (`ui-monospace, "SF Mono", Menlo, monospace`) for code, file paths and terminal-like build output.
- Sizes: controls are 24, 28, 32, 40 px tall (`h-6 h-7 h-8 h-10`), radii `rounded-md rounded-lg rounded-[10px]`, text mostly `text-sm` (14px), `font-medium` labels, `text-xs` for meta.
- Fills use the alpha scale: `bg-alpha/5` subtle surfaces, `hover:bg-alpha/10`, `border-alpha/10` borders. Disabled is `opacity-40`. Inactive tabs are `opacity-40` going to 100 on hover/active.
- Variants: primary (solid `gray-950` on `gray-50`), subtle, outline, ghost, plus colour variants (`blue-500`, etc.) each with solid, `-subtle` (`/10` fill), `-outline`, `-ghost`.
- Motion: `duration-100` colour transitions, `--ease-fluid: cubic-bezier(0.08, 0.82, 0.17, 1)`, a slow `pulse-subtle` (3.5s, to 0.7 opacity) for "agent is working".
- Shadows: `--shadow-dropdown: 0 4px 24px -8px rgba(0,0,0,0.2)`, `--shadow-card: 0 8px 16px -8px rgba(0,0,0,0.1)`.

Recolour with **Catppuccin Mocha**. The app is dark only for this prototype. Map the Hooman scale so existing class names keep meaning (in Hooman's dark theme `gray-50` is the darkest background and `gray-950` is the text colour):

| Token | Mocha | Hex |
| --- | --- | --- |
| `--gray-50` | crust | `#11111b` |
| `--gray-100` | mantle | `#181825` |
| `--gray-200` | base | `#1e1e2e` |
| `--gray-300` | surface0 | `#313244` |
| `--gray-400` | surface1 | `#45475a` |
| `--gray-500` | surface2 | `#585b70` |
| `--gray-600` | overlay0 | `#6c7086` |
| `--gray-700` | overlay1 | `#7f849c` |
| `--gray-800` | overlay2 | `#9399b2` |
| `--gray-900` | subtext0 | `#a6adc8` |
| `--gray-950` | text | `#cdd6f4` |
| `--alpha` | white | `#ffffff` |
| `--blue-500` | blue | `#89b4fa` |
| `--blue-600` | sapphire | `#74c7ec` |
| `--cyan-500` | sky | `#89dceb` |
| `--teal-500` | teal | `#94e2d5` |
| `--green-500` | green | `#a6e3a1` |
| `--yellow-500` | yellow | `#f9e2af` |
| `--orange-500` | peach | `#fab387` |
| `--red-500` | red | `#f38ba8` |
| `--maroon-500` | maroon | `#eba0ac` |
| `--purple-500` | mauve | `#cba6f7` |
| `--pink-500` | pink | `#f5c2e7` |
| `--lavender-500` | lavender | `#b4befe` |
| `--rosewater-500` | rosewater | `#f5e0dc` |
| `--flamingo-500` | flamingo | `#f2cdcd` |

Pick which of crust/mantle/base is the window background versus panel background versus raised surface, and keep it consistent. Solid colour buttons on Mocha need dark text (`#1e1e2e`), not light. Ghostty's default Mocha look is the reference for the overall temperature: dark blue-grey, soft pastel accents, nothing neon. Use accents sparingly: one accent for the agent (lavender or mauve is a good fit), one for transport/play state (green), one for warnings (peach) and errors (red), and let musical tools pick their own single accent so instances are telling apart at a glance.

## Constraints

- React 19 + TypeScript + Vite + Tailwind v4, installed with **bun**. Strict TypeScript. Keep dependencies minimal: `lucide-react` for icons, `clsx` + `tailwind-merge` for `cn`, `class-variance-authority` if you want variants. No component library; write the primitives yourself, adapted from Hooman's. Do not depend on the Hooman repo at build time; copy what you need.
- Fake data only. No audio, no real agent, no persistence. Cheap interactivity is welcome where it helps judge the feel (dragging a slider, switching a tab, hovering a state, toggling a build status), but do not build features.
- Desktop window sizes only: design for roughly 1440x900 and check 1200x800. No mobile.
- Include a **primitives gallery** page or route showing every primitive with all variants, sizes and states (hover, focus, disabled, active), so the design system can be reviewed and locked on its own.
- Include at least one state of the agent **mid-build** and one where a build **failed** and the previous version is still running.
- Show at least one tool instance open in **two views** at once, and make it clear that they show the same instance.
- Accessible controls: visible focus rings, labelled inputs, keyboard-reachable buttons. This is a real requirement of the product, so the prototype should demonstrate it.

## Deliverables per version

In your version folder:

- A runnable app: `bun install`, `bun dev`, `bun run build`, `bun run typecheck` all work. Vite build must pass with no TypeScript errors.
- `README.md`: how to run, what screens exist, and a short list of the design decisions you made and why (goal, constraints, what to look at). Keep it under a page.
- `screenshots/`: PNGs of the main window in the default state, the build-in-progress state, the failed-build state, and the primitives gallery. Take them with `agent-browser` (available on PATH; run `agent-browser --help`) or Playwright's bundled Chromium (already in `~/Library/Caches/ms-playwright`). Set the viewport to 1440x900. Look at your own screenshots and fix anything that looks off before finishing.
- Keep the folder self-contained: its own `package.json`, `node_modules`, `tsconfig.json`. Do not touch anything outside your version folder except reading.
