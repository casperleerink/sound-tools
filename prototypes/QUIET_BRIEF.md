# Quiet revision brief (version C)

Casper reviewed prototypes A and B on September 10, 2026 and found both **too busy**. Version C is a revision of B's core with one rule: **every element must earn its keep**. Hide everything that is not strictly necessary. The reference for the target feel is Hooman Studio and Hive: lots of whitespace, nothing crowded, one accent, calm typography. Look at `/Users/casperleerink/hooman/hive/shot-light-reviewer.png` (a Hooman design prototype) for the amount of air, the 16px body text and the restraint. Audio tools are usually crowded; that is not a good design choice and we are not copying it.

Direct quotes from Casper:

- On the agent's build activity: "I would maybe only show 'Building Polyrhythm', that's literally the only text. You can have some history. But even Codex doesn't do that either, it just says 'worked for 24 minutes' and then gives the result."
- "Things you definitely shouldn't do are texts like: 'waits for the build, stops playback' or 'waiting for the build, you can keep playing the current version'. Any of those kind of texts should be 100% removed."
- "Things like a legend for lines is another example, it's just not necessary. That information can be somewhere but shouldn't be on the main screen."
- "I do like the transport more in the canvas prototype. It is better but can be cleaned up as well."
- "Let's just focus on the core parts for now." Canvas versus panes is not decided; the canvas is kept only because B's core is the base.

## What to change, concretely

**Agent sidebar**

- A turn is: the composer's message, then the agent's result text. Nothing else by default.
- While the agent works, show one line only, for example `Building Polyrhythm` or `Editing Polyrhythm`, with a slow subtle pulse. No tool-call rows, no diff counts, no timings, no progress bars, no "reload" rows, no explanatory sentences.
- After a turn, that line collapses to a quiet one-liner like `Worked for 12 s` (muted, small), which can expand on click to show the history: reads, edits, build, reload. Expanded history is allowed but must be secondary and off by default.
- A failed build shows in the same one line: `Build failed` in red, and the agent's text explains in one short sentence. No banner boxes, no "previous version still running" prose anywhere. If that fact must exist, it lives in the expanded history only.
- Result text is short and plain. No timestamps per message; one session line at the top at most.
- Composer: input, model name, send. Drop the "Plan" toggle and any keyboard hint unless it is a placeholder inside the input.
- Sidebar header: just "Agent" and one new-session button. No history or collapse icons unless they can be a single overflow menu.

**Transport**

- Keep B's floating pill at the bottom centre, then reduce it: play/pause, stop, position. Duration only if the project has one. The seek strip can stay if it is quiet.
- Build status and audio device leave the pill. Device selection moves to a menu behind the project name. Build status is the agent's business; show it in the sidebar only, plus at most a small dot in the pill when a reload is about to happen.

**Canvas and chrome**

- Remove the wire legend, the zoom cluster and the percentage. Fit on load, zoom via trackpad and keyboard only.
- Top-left: project name as a quiet menu trigger and nothing else. Undo/redo are keyboard shortcuts; "Add tool" goes into that menu or an empty-area affordance.
- Fewer cards. Show three instances at most: the polyrhythm instance open once, the additive drone, and the mixer. Drop the second views on screen; a "second view" can be demonstrated in the primitives gallery or an extension preview route instead.
- Cards: larger internal padding (16px minimum), fewer readouts, no meta chips in headers. Port labels appear on hover only. Wires stay but with one weight and no dashes; audio, event and modulation can differ by opacity or colour only. Remove the dot grid or make it barely visible.
- Remove the prototype state toggle from the main UI; keep `?state=building` and `?state=failed`.
- Remove the "Primitives" link from the main UI; keep `#/gallery`.

**Typography and spacing**

- Base text 14px is fine for controls, but agent conversation text is 15 or 16px with 1.5 line height and wide margins, like Hive.
- Use an 8px grid for layout spacing, 4px for inside controls. Card and sidebar margins at least 24px.
- Colour: one agent accent (lavender), one play accent (green), red for errors. Tool instances keep one accent each, used only on the dot and wires, not on fills.

**Extensions**

- Do not build new extensions. Simplify the three you show so they match the new density: hide secondary parameters behind a "more" disclosure or a second view that is not open by default.

## Deliverables

Same as the shared brief: `bun run typecheck` and `bun run build` pass, `README.md` under half a page, `screenshots/` with default, building, failed-build and gallery at 1440x900, reviewed by eye and fixed before finishing. Also add a short `screenshots/COMPARE.md` listing what was removed relative to B, one line per item.
