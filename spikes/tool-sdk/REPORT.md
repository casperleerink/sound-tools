# Spike: how a tool should be written on Rune

SPEC.md built twice on Rune, with the same theme, the same screen and the same song as
`../rune-vs-gpui`: `ts/` (TypeScript in Bun, as today, the view a plain function) and `luau/`
(Luau scripts inside the app, `rune-luau`). `rune` below is the hand-written Rust app of
`../rune-vs-gpui`: the floor.

## Performance (`../rune-vs-gpui/measure.py --apps ts,luau,rune`, CPU includes Bun)

8 tracks, 32 clips, median of 3:

| scenario | ts | luau | rune | GPUI (earlier) |
| --- | --- | --- | --- | --- |
| idle | 0.1% | 0% | 0% | 1.4% |
| play | 6.2% | 6.0% | 5.9% | 23% |
| zoom (tool view runs every frame) | 16.9%, 51 fps | 13.3%, 55 fps | 9.9%, 55 fps | 24–27% |
| edit (a record changes every 100 ms) | 13.1% | 8.0% | 6.2% | 16–27% |
| click on solo, p50 | 14.5 ms | 2.1 ms | | |
| memory | 126–154 MB | 102–105 MB | 96–99 MB | 79–80 MB |

32 tracks, 512 clips, median of 2: play 13.8 / 13.3 / 13.7%; edit 20.0 / 15.3 / 15.7%; zoom
47.5% at 49 fps (10 MB/s over the pipe) / 38.9% at 43 fps / 24.2% at 47 fps. Memory: ts
236–315 MB, luau 157–191 MB, rune 111–133 MB.

- What the composer does (play, edits, clicks) costs about what hand-written Rust costs, in
  both. Both are well under GPUI today.
- The weak spot is a tool view that runs every frame on a big song (zoom). Keep zoom in the window,
  as scroll already is, and no scenario runs tool code per frame.
- Click: Bun answers in ~0.8 ms (measured inside); the rest of ts's 14.5 ms is the spike's wake
  path, timed from press to tree, while luau's is from request to frame. Not a fair pair.
- Bun costs ~30 MB at template size and 100+ MB on a big song (the state lives twice).

## Working with an agent

Two fresh agents, each given only its variant's `README.md`, added a "Mute all" button and a
clip-count badge component, and proved it worked.

| | ts | luau |
| --- | --- | --- |
| Time, tool calls | 51 s, 12 | 102 s, 25 |
| Failed checks | none | 2: a wrong SDK type passed the check and crashed at run time; a change the window dropped with no message |
| Test with no window | `bun test` in the project | none in the project (the Rust app's tests); proved with `--script` and `--tree` |
| Type check | `tsc`: token names, props, a press without a key | `luau-lsp` (separate install): token names, fields; not a misspelled prop |

Both forks' tool code ran on its first load; their Rust windows took 2 and 3 tries. The ts agent
wished the README had: the `--script` format, that a `memo` must list what it shows, and how to
test two presses (`apply`).

## Size

| | ts | luau | today (GPUI) |
| --- | --- | --- | --- |
| SDK an agent reads | 221 + 6 lines | 150 lines | 6,500 lines, 329 exports |
| Agent doc | 422 words | 336 words | ~6,300 words |
| `ui/` components | 39 lines | 85 lines | in Rust (`crates/typescript/src/card`) |
| The tool (song page) | 155 lines | 240 lines | ~3,400 lines of `.tsx` (more features) |
| Window side | 725 lines Rust + 34 host | 482 lines Rust | ~23,000 lines in `crates/typescript` |

## Verdict

**Keep TypeScript in Bun; replace the view model.** Luau is a little faster and lighter, but
the agent did half as well with it, its types catch less, and moving to it would also move the
sound and every behaviour to a second language. Bun's costs are memory and one pipe, and both
are fine at DAW-template size.

What the spike says a tool's view should be:

- `view(state)`: a plain function, run again after every change; `memo(key, deps, build)` for
  big parts. No signals, no `For`/`Show`, no rule about where a value may be read.
- Free layout from `row`, `col`, `box`, `text` and `shapes` (in units of the tool's own, so a
  zoom stretches them).
- A design system as data: `theme.json` tokens (colour, text style, space, radius, shadow),
  typed as literal unions, and components as source files in the project's own `ui/`.
- Records change through `change(draft => …)`; what moves every frame is bound by name
  (`live`, `level`, `drag`), never by tool code.
- Checks: `tsc`, `bun test` with `render`/`find`/`press`/`apply`, and the window's `--snap`,
  `--tree`, `--script`.

Not tested: the sound side (unchanged), tool memory such as a selection, menus, text input,
plugin windows.

## Agent eval: today's SDK against the new one (`tooling/agent-eval`, `--label gpui,next`)

Opus 5.5 builds 8 tasks (4 new tools, 2 DAW template changes, 2 follow-ups); a council (Opus 5.5,
GPT 6.1 Sol) checks hidden criteria through the app and scores UI and UX 1–5. One run each;
new SDK at `t3/rune-window-slice` 7f461963.

| | today (GPUI) | new (Rune) |
| --- | --- | --- |
| Criteria pass / fail / unverifiable | 54 / 2 / 8 | 58 / 3 / 3 |
| UI / UX, mean | 3.8 / 3.3 | 3.1 / 3.1 |
| Build time, cost | 1,535 s, $9.54 | 2,463 s, $16.64 |

- Works as well, and more can be checked without a window (`--act` drags and presses).
- UI is lower for the same reasons in every task: knobs show no value, control sizes off the
  scale, solid green for "chosen", cards stacked tall instead of the device grid, a second Play
  button. That is missing components and blocks (SPEC's layers 3–4), not the primitives.
- One real SDK bug: a drag on a pressable step also toggles it (sequencer follow-up).
- Builds are 60% slower and dearer; the builders check more (`--snap`, `--tree`, `--act`).
