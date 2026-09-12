# Sound Tools prototype C: quiet

Revision of prototype B with one rule: every element on screen must earn its keep. Same core (canvas, agent sidebar, floating transport), fewer things, more air. React 19, TypeScript, Vite, Tailwind v4, fake data only.

## Run

```
bun install
bun dev            # http://localhost:5176
bun run typecheck
bun run build
```

## Screens

- `/` the window. `?state=building` and `?state=failed` switch the agent story. There is no toggle in the UI.
- `#/gallery` every primitive with all variants, sizes and states.
- `#/ext/<type>` one extension with all its views side by side; this is where a second view of an instance is shown. Types: `polyrhythm`, `additive`, `mixer`.

Screenshots at 1440x900 are in `screenshots/`: `default.png`, `building.png`, `failed-build.png`, `gallery.png`. `screenshots/COMPARE.md` lists what was removed relative to B.

## What changed and why

- **Agent sidebar.** A turn is the composer's message, one activity line, the agent's text. While working the line says `Building Polyrhythm` and pulses; after, it collapses to `Worked for 12 s` and opens on click to show the steps. A failed build is `Build failed` in red plus one sentence. Conversation text is 15px at 1.5 line height with 24px margins. Composer: input, model, send.
- **Transport.** Play/pause, stop, position, a hairline seek strip, duration. Build status and the audio device left the pill; one lavender dot appears while a reload is pending.
- **Chrome.** The project name is the only thing top-left. Its menu holds add tool, undo/redo, the output device and the project folder. No zoom control, no legend, no grid; fit on load, pinch or `mod` `+`/`-`/`0` to zoom.
- **Cards.** Three instances, one view each, 16px padding, name and view tabs in the header, close on hover only. The instance accent is on the dot, ports and wires only; the body is neutral. Wires are one weight and differ by opacity.
- **Extensions.** Polyrhythm, Additive and Mixer trimmed to what a composer needs while playing; the rest lives in each tool's second view.
