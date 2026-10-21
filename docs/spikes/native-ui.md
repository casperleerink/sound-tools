# Spike: a small core and a TypeScript kit that builds any tool

## Goal

Sound Tools becomes a small core plus a TypeScript SDK with few concepts and many Rust building blocks used as children. They compose into any kind of tool. The DAW is not built in: it is a project written with the SDK, like any other.

## Decided constraints

- **Few concepts.** Anything that can be a child or a helper is one. Change the cut when the work shows a better one; every new concept needs a reason.
- **Rust builds blocks, TS owns structure.** Rust is welcome for blocks the SDK uses as children: a reverb, a synth, a sampler, the plugin host, timeline, recorder, audio and MIDI in and out, meters. A block composes through ports and knows no DAW idea. Rust never owns how a tool or project is organised: no tracks, clips, mixer layout, transport bar or views that decide a workflow.
- **The core knows no music beyond the clock:** position, tempo map with its sections (a change of time signatures moves them), start, stop, jump, a repeat range. What play or record means, the transport bar and every musical key are SDK or project code. The app keeps only app and file shortcuts and machine settings.
- **Motion never crosses to Bun; structure updates are fine-grained.** The mechanism is open: change it when a measurement says so.
- **TS tools run unchanged on any OS, with no build.**
- **Typed end to end.** Misuse is a type error, or an error that says the fix.
- **Possible, not easy.** The SDK must make a full arrangement possible and leave room to improve it. The arrangement project is a work of its own.
- Branches: `spike/native-ui-solid` (current), `spike/native-ui` (React-like, round 3a, reference only). Never main.

## How to verify

- A project of TS tools works almost like main's Rust arrangement: tracks with instruments and effects, note and audio clips, a note editor, automation, a mixer with sends and meters, the master, transport and loop, MIDI and audio recording, file drop, device cards. Fit tempo is out of scope. **Met:** `templates/daw`, driven by `crates/runtime/tests/window/tools/{song_view,template}.rs`.
- Nothing in the runtime knows the arrangement. **Met:** the Rust arrangement is deleted.
- It feels as smooth as main's arrangement: numbers below.
- Agents still write new tools from the docs: eval below.
- Every new concept or block has window or render tests.

## Decisions

Settled during rounds 1–14; the round-by-round log is in git history. The lasting ones are in [ARCHITECTURE.md](../../ARCHITECTURE.md).

**The UI model**
- **GPUI draws, TypeScript decides, as in React Native.** Bun runs a tool's code; the window keeps a node table and draws it.
- **Fine-grained, as in Solid, not a tree per change.** Round 3a (`spike/native-ui`) redrew the whole tree per change: a click at 20,000 notes sent 1 MB; with ops it sends 0.7 KB. A component runs once; a function given as a prop is a binding that runs again when what it read changes.
- **Our own reactive runtime in `sdk.ts`.** Rejected: Solid with its Babel transform (5–8 MB in every project, about 100 ms cold start, a compile step agents cannot see); Solid without the compiler (DOM-only). Owning it lets misuse throw with the fix ("x={() => note.start}, not x={note.start}").
- **Bun hears only what its bindings read** (read tracking), so a knob turned natively costs Bun nothing.
- **Motion is native:** live values (`now`), `Space` units, scroll, zoom and culling, `Group` `moves`/`grows`, clipping, declarative drags with snap and grid. Code hears a drag once, at its end. `onMove` was removed for crossing on every pointer move.

**The shape of a project**
- **Realistic means many small records:** a song of tracks of clips, not one 20,000-note field. Children nest at any depth; a pattern name (`"ret-*"`, `"*"`) stands for many children of one tool.
- **The concept cut:** tool (record, memory, tick), children, typed ports and connections (signal, number, bang, note), signal, view, update, clock. What became a child or a helper: the sequencer (`timeline` with `loop`), meters (`meter`), mixing (`fader`), automation (`lane`), audio clips (`region`), recording (`recorder`), the transport bar (`<Transport/>`), Knob, Steps, Meter, Pad (TS components), the canvas (`Space`).
- **Two hooks with reasons:** `onTake`, because what a take becomes is structure, which TS owns; `takes(port)` slots, because a list of tools would leave out the project's own instruments and plugins.
- **Children reach all code filled with defaults**, one shape, no casts. A behaviour sees what is inside its children at any depth, and the fields it reads there decide when it is asked again.
- **Each take of sound is a recording of its own over the one input**, so a second recorder, or the next take at a loop, starts at once instead of waiting for the one before.
- **What a performer sends for later waits in a bounded queue of its block** (signals, recorder, timeline alike) and lands at its frame; a send for now drops what waits.
- **The window names no musical key:** the computer keys leave a focused tool the keys it names.
- **One road to a built-in effect: a child.** `devices.reverb(...)` inside a `sound` was removed: a second road with its own types, checks and engine ports.
- **Clock in the core, everything musical outside it.** `clock.play/pause/stop/jump(to, { atTick })/setRepeat/setClick/update`; the transport bar is ~50 lines of SDK; space plays only where a tool's `globalKeys` takes it.
- **Device in and out stay `project.json` connection ends.** An `audio-out` child would need the parent's own sound as a source, which ARCHITECTURE rejects (`ownSound`).
- **`--only <path>`** plays one instance's sound by rerouting `project.json` in memory.
- **Solo is a cable:** a `lane` holding 1 into every other track's fader `mute`, a number, so a volume lane plays on under it. Pre-fader sends are the return's choice; a silent track's are muted too, as main's were.
- **No compiled extensions.** Rejected: rebuilding the app, wasm or dylibs from a project: Xcode tools or MSVC and a build when a project opens.
- **Templates:** a folder of `templates/` is a template; Bare and DAW. The Rust arrangement, `arrangement_wiring` and fit-tempo are deleted.

## Numbers

M1 Max, release-fast. Repeat with `projects/render_cost.rs` and the ignored tests in `window/tools/performance.rs`. The Rust arrangement column is from round 12, before it was deleted.

| | TS song | Rust arrangement |
| --- | --- | --- |
| Render, 12 tracks / 150 clips, same content | 26x real time | 28x |
| Scale project 100 × 100: open | 3.3 s, until drawn | 1.8 s |
| Scale project: frame playing | 2.1 ms | 1.1 ms |
| Scale project: frame scrolling / far zoomed out | 2.0 / 1.9 ms | 3.4 / 3.5 ms |
| Clip drag, per move | 2.5 ms (12 / 150), 3.5 ms (24 / 600), 0 Bun requests | 3.9 ms |
| Select a clip / release a move, until drawn (12 / 150) | 12.4 / 14.0 ms, Bun 1.0 / 1.5 ms | |
| Space until the clock plays (12 / 150) | 17 ms | |

"Until drawn" includes about 10 ms of the test's polling. `tooling/perf` and the agent evals now build DAW-template projects, so their numbers start a new baseline.

**Agent eval** (round 12): `daw-arp-builder` (an arpeggiator before a track's synth, $1.90), `daw-return` (a return with a long reverb, $0.18), `ui-chords` (a chord player card, $0.75): all passed. Round 14c ran them again on the trimmed docs.

## Open gaps

- Opening grows with the song: about 0.8 µs per binding in Bun, 3.3 s at 10,000 clips. Needs a leaner reactive core or rows made only when they show.
- A playing frame costs about twice the arrangement's on the scale project.
- Template features: dragging a ruler marker, a double-click that clears a lane, a foldable returns section, fades as dials rather than handles, a clip moved to another track leaves its automation behind.
- A clip moved to another track is a delete and a create: a record's id is its path.
- Copied notes and copied clips are two clipboards.
- `"*"` and named children cannot be optional.
- A `takes("notes")` slot accepts any tool name in `update`'s types; only the runtime says one takes no notes. The built-in tools' entries do not say what they take.
- Inline TS tools in Rust tests are not type-checked: some are wrong on purpose and need a mark first.
- Headless snapshots show no meter motion.
- Recording paths with no test that fails without them: the old clock closing a take at a tempo change, missed events and falling behind reported, a take held while Bun restarts, questions numbered per Bun.
- `now` is exported untyped and on the context; they are one object.
- Two new-project flows (restart on a folder, open from the start window) share one write.

## Round 14c

Docs cut to decisions and matched to the code. Words before → after: the spike doc 15,289 → 1,400; the TS agent doc 6,040 → 5,826; DESIGN 1,535 → 1,402; ARCHITECTURE 4,383 → 4,762 (it took the spike's decisions); README 2,438 → 2,422. Device docs place records where the DAW template keeps them. Nine unreferenced mockups and about 30 comments and fixtures naming the arrangement went.

**Agent eval on the trimmed docs**, one run each, all passed: `daw-arp-builder` (33 turns, $2.19), `daw-return` (9 turns, $0.23), `ui-chords` (17 turns, $0.80).

## Round 16: the song looks as the arrangement did

Casper asked for the DAW template to look almost exactly like main's arrangement, transport in the title bar. Side by side with main's snapshots in `target/look-compare/` (main left).

SDK additions, each generic, none a new concept (all are view elements or style):
- `TitleBar`: a div of the main view that the window shows in its title row. The transport is project code, so the title row needs a slot for it; any page can use it.
- `Button`, `Toggle`, `Icon`, `PeakMeter`: the app's own controls and meter, drawn by `crates/ui`. Shapes could imitate them, but not exactly, and every tool should look like the app.
- Theme colours by name (`"gray-700"`, `"alpha/10"`), side paddings, `font: "tabular"`, `weight`, `hover`, `keep`, `truncate` on `Style`; `beats` and `note` readouts.
- `clock.computerKeys` / `setComputerKeys`: the computer keys are a switch of the session, as the click is, so a tool's transport shows and turns them. The live input keeps no view of its own; cmd-K still turns them.
- A page has no 24-point padding; its view decides.

Round 16b compares the same content: `snapshots/look.rs` draws the piece of main's snapshots in the template. A track is a synth and its chain, as main's was, so a filter is an effect like any. Master opens main's master panel; Mixer is a button on the master row. The editor, lanes, pickers, knobs and strips match main.

More SDK, none a new concept:
- `Space` `fit`: a range fills its width once laid out. An editor opens on its clip and TS knows no window size.
- `untrack` is exported: what a component takes once as it opens.
- `DeviceCard` `onTitle`: the title is a menu, as main's slot pickers were, for any device in a slot.
- Menu items take `icon`, `detail` and `group`, and `openMenu` notes. `offers()` gives icons and the pickers' notes. A tool's picker then lists devices as the window's do.
- A `pan` readout; `Knob` draws as the app's; `#rrggbb/NN` colours.

Round 16c adds the audio scene of main (Voice and Guitar, the second voice region chosen) and closes the menus:
- A `region` has a card, drawn in Rust as main's Clip card: its file on the waveform display, the trim lines, the gain line with its fades, Gain, Fade in and Fade out. It is a block's card, as a reverb's is; the template shows it first in an audio track's panel, then a hairline, then the track's effects. `Views::register_json_card` gives a tool whose record is JSON a card.
- An audio track (`tape`) has a `chain` and a `title`, as a track has. A chosen region shows its fade and gain handles on the timeline. Its panel has the input select, `In 1 ▾`, which writes the recorder's `channels`.
- A menu opened by a press of a div, a button or a card title hangs under it and moves into the window, as main's selects do; after a right press or a press of a space it opens at the pointer. `openMenu` takes `width` and `maxHeight`. `offers(takes, { at })` answers `here`, the offer at that child, which a picker ticks.
- The master's limiter has its power icon (`limit_off` on the song); Mixer is an icon button.

Still different: main's pill showed no position strip until the first change after opening; the template's shows it whenever the song has an end. Main's limiter on the master had no lookahead select; the limiter's card has one, a feature of the device. Text and lines differ by a fraction of a point in places.

Round 16d adds the armed and recording scenes of main (`look-armed`, `look-recording`) and closes the rest:
- `now.input(channel)` and `<PeakMeter input={[0]} />` read the level of the audio input, natively: the window keeps it at every poll while the input is open. An armed track shows it in its header and on its strip, as main did. Why: "the composer sees the level" was promised by the recorder and no view could show it.
- A `Path` shape fills between its points. Why: main shaded outside a fade, which no rect or line can draw. A wave's `level` scales it along its width, so a region's waveform shows its gain and fades as main's did; a band is one smooth shape, not a quad per column. A filled path ends its layer, as GPUI draws the paths of one layer over each other's boxes.
- A button's `opacity` (main's quiet close buttons) and a div's `lineHeight` (a card's two-line note).
- A take while it records is drawn as a region: over what it covers, with a red border and its sound so far.
- Fixed in passing: clips were a point short, the playhead half a point left, the panel a point tall, the hairline under the master row showed with nothing under it, the editor striped its white keys, and card titles sat a point left.

## Round 17: Casper's first test

- Clip previews drew their notes twice. A list inside a row of another list took only the nodes of its first draw along when that row went; what it drew since stayed, stale. A list or a function child now takes its own nodes out as its row goes.
- Menus scroll past 420 points unless they say: the design system's height, as wide menus have its width. `openMenu` takes `search`, a field over the items, as main's lane picker had.
- `numbers()` gives each number its `label`, as its device names it (a plugin's parameter) or its field in plain words. The template's Add lane menu lists them as main did: Track, then a group per device.
- Every node that hears the pointer has `onPress`, `onRelease` and `onClick`. Why: main's repeat bar is clicked on and off and dragged, and code heard a press only as it went down; a pad needs its release. The ruler now does what main's did: a click on the repeat's bar turns it on or off, its body moves it, an end moves that end; a double click adds a section.
- What shows under the timeline is one slot of the song's memory: a track's panel, the master's, a clip's notes or the mixer. A mixer strip has a track panel's knobs and fader; strips keep the room of the most sends, so faders line up; the master's strip is on the right, its limiter in its panel.

## Round 18: updates of a template

- A project from a template keeps its code in `.template/` (see ARCHITECTURE, Templates); a later release updates the files the project never changed and leaves the rest to an agent merge from a notice. A project made before this kept nothing and is left alone: no base tells its edits from an older template's.
- A tool's `migrate` hook waits for a template change that needs one: children are filled with their defaults, so a field a newer template adds reads as its default in an older record.
