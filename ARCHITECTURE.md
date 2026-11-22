# Technical architecture

The technical decisions, why we made them, and what we rejected. How something works is in the code; each section names where. A line belongs here only if the code cannot say it.

## Terms

| Term | Meaning |
| --- | --- |
| Extension | A Rust crate that registers tools, their feeds and facts, and agent docs with the core. Bundled extensions use the same API a user extension would. |
| Plugin | A third-party audio plugin (CLAP, VST 3). Never our own extensions. |
| Tool | A composer-facing capability an extension defines. In code, a tool is its saved state type. |
| Tool of the project | A tool a project defines in TypeScript under `extensions/`, with no build. |
| Block | A tool written in Rust that tools of the project use as a child: `timeline`, `fader`, a reverb, the plugin host. Not the blocks of a project's UI (`extensions/ui/blocks/`). |
| Template | The files a new project starts from: a folder of `templates/`, or one the composer saved. |
| Instance | One use of a tool in a project, with its own record. |
| Record | The saved JSON state of one instance, one file in `state/`. |
| Behaviour | A function from the state of an instance to what it needs in the engine: processors, updates, connections, ports. |
| Asset | Bytes under `assets/` the core never looks inside: audio files, raw takes, plugin states. |
| Problem | A part of the project that is not live, listed in `problems.txt`. Never a reason to refuse the rest. |

## Shape of the system

- One process, `sound-tools`, opens one project folder: as a window, headless, or read-only to inspect, render or analyze.
- Agents only edit files in the project folder, and the runtime applies them live: there is no edit API.
- Every extension is compiled into the one binary; `project.json` enables them by name.
- Rust for the core, the extensions and the window; TypeScript for tools of the project. Audio and saved data never depend on the toolkit.
- The project, the window and every plugin handle share the main thread, which plugins require. Rejected: a control thread of its own, which splits the project from its plugins.
- macOS 13 or later is the main platform: Rune draws with Metal 3. No plugin windows on Linux: plugins there need an X11 parent and a host run loop.
- System programs come from the system, never the `PATH` (`sound_core::process`).
- Running Sound Tools as a plugin inside another DAW is out of scope.

## The core knows no music

This is the main rule of the codebase.

- The core has no track, clip, note, effect, plugin or audio file type, and depends on no workspace crate (`tooling/workspace-rules`).
- Three layers: the core knows no music; extensions build blocks in Rust through core hooks, and a block names no DAW idea; tools of the project, in TypeScript, own the structure (tracks, clips, a mixer layout, what play and record mean), so a project can change them. The app knows no extension but where it registers them.
- Tools find each other by port names, not by type, so a parent depends on no instrument, effect or plugin.
- The core does own the musical clock, and nothing more musical: position, tempo map, play, pause, jump and a repeat range. Nearly every tool and request talks in bars and beats, so one clock beats one per extension. Sections (named bar ranges) live next to it in `project.json` for the same reason, not in a tool.

## Tools, instances and behaviours

`crates/core/src/project`.

- A behaviour is declarative: it runs for every valid state from every source and declares everything; the core sends only the difference, so it cannot leak processors.
- Refusing is for invalid state. State that is valid but cannot be live now (a missing plugin or file) is a problem, and the rest plays.
- References are saved ids that keep nothing alive and may arrive before their target; data of a missing extension stays intact.
- A service outside the project that can now do more (a plugin scan, a background render, a file arriving under `assets/`) makes the core run a behaviour again with the record it has: no write, no point (`runtime::tick`).

## Project storage

A project is a folder of JSON records and assets. Together with its extensions it is the whole piece. `crates/core/src/project/storage.rs`; the layout an agent reads is `agent_doc.md` beside it.

- The folder tree is the ownership tree, so one parent per child and no cycles come for free. The id of an instance is its path; display names live in the record, so a rename never moves a file.
- A tool decides its form (folder or file) for good: a path that changes under an agent leads to ignored writes and deleted records coming back.
- The format is chosen for agents and git: adding a part is one new file, moving a clip to another track moves a file. There is no instance index.
- No schema versions and no migration framework. Old records must load unchanged: a new field is optional with a default that sounds as before, and the runtime does not rewrite a file that did not change.
- Only an invalid `project.json` stops a project from opening. A change of `extensions` applies on reopen.
- What belongs to one machine stays out of the folder (plugin scan cache, window positions, audio devices, agent threads and settings), so a project in git does not change when opened on another laptop.
- Recordings and imported files are never written over, deleted by the runtime or part of history, so an edit cannot lose one.
- Files are written to a temporary file and renamed into place. No `fsync`: it blocked undo of a large delete for most of a second; power loss is left to git.
- One runtime per project holds `.sound-tools.lock`. Inspect and render open read-only without it, next to a running window.

## The live folder and editing

`crates/core/src/project`.

- There is one state application. Window edits, file changes, loading, undo, redo and cancel all apply a group of changes, whole or not at all. Loading is applying from empty.
- The watcher only says which paths changed; the runtime reads them again and applies the difference, so edits, new files, deletes and moves take one road. The runtime groups raw events itself, because a debouncer can split a burst.
- Last write wins everywhere: file edits, open drags, undo, redo, cancel. No field merge, no conflict rejection: do not reopen this as a sync problem.
- An agent request is one point of history (`Project::begin_request`). Rejected: adding writes that come after a composer's edit to the older point: undo would go out of order.

## Project history

`crates/core/src/project/history.rs`.

- **One system.** Undo, redo and the History tab are moves through the same points; there is no undo stack. Only files are history: playback, arming and selection are not.
- **A git repository of its own** in `.sound-tools/history`, written by hand (loose objects, refs, a detached `HEAD`), so plain `git --git-dir … log --all --graph` reads it. Rejected: the folder's own `.git`, which an agent's git commands rewrite; jj; a git library, heavier than the three object types we write.
- **Kept:** `project.json`, `state/` and the project's code in `extensions/`, without what the runtime writes there. Assets are never written over, so need no history.
- **Same state is the same bytes.** Undo writes a point's files back byte for byte, and the project follows them as it follows an agent's write: a file that does not load comes back, and the sound plays the last good version. Rejected: applying in-memory records, which rewrote an agent's file in our layout.
- **A point** is a finished edit, a run of a tool once quiet, or outside changes. Outside changes close together or of one request replace the point before, and one that takes back what it wrote leaves none. A drag in memory never reached disk, so it is no point. Before any move, what the folder holds that history has not seen is saved first, so nothing is lost.
- **Going somewhere is never a point.** The next change after it starts a version. After undo, it does only when the undone work was big; small undone work is marked undone, kept in git and findable. alt-cmd-Z undoes as going back does.
- **Lines are explicit** in `lines.json` next to the repository: each version or undone run with its start and tip, number and name. Rejected: working out lines from "first child" order, which needs the whole history and moves lanes when undone work is kept.
- **Merge is per file**, a point with two parents. Rejected: merging inside a JSON record, a sync problem (see above).

## Agent context

`crates/core/src/project/generated.rs`.

- One map, `AGENTS.md`, and one file per doc in `agent-docs/`, because one big doc stopped scaling. Rejected: Claude Code skills, no better in an eval and needing settings agents run without.
- A doc never outlives its tools and holds nothing of the machine, nor what an agent reads in the SDK, a field table or a problem.
- Writing tools is one doc, `tools.md`; the doc comments of `sdk/` are its reference, so a type and its words cannot drift apart. Rejected: five docs by area, 6,300 words that retold the SDK.
- The composer's instructions are plain text edited in the sidebar: `instructions.md` travels with the project; one in the support folder covers every project.

## Audio engine, transport and time

The threads and the audio thread rules are in [ENGINEERING.md](ENGINEERING.md). `crates/core/src` (`engine.rs`, `processor.rs`, `clock.rs`, `transport.rs`).

- The audio thread plays at real-time priority, also on Linux. Without it a slow laptop played a piece at 2.5 times the cost and dropped out.
- Every audio port is stereo: no channel negotiation, no half-connected cable, and a plugin's stereo output fits one port.
- Time signatures are runs of whole bars and sections start at a bar number, so neither falls inside a bar. Rejected: ticks.
- Changing the time signatures moves bar lines and sections, never notes. Rejected: keeping bar positions, which moves every clip and breaks those off the bar lines.
- A rit. or accel. is a tempo change with `to`; a fermata divides its beat's tempo. Both play as 32nd-note steps, so conversions stay exact. Rejected: a curve (no exact inverse); steps a tool writes (the mark is lost).
- Bar math lives in `TimeSignatures`, so no extension assumes a fixed bar length.
- The loop, and a jump at a tick, go back on the frame of their end, and each processor ahead of the device on its own frame first, so processors need no loop code. Rejected: a seek when the device gets there, which with latency cut what rang one latency early: a click on every repeat.
- The engine runs every processor every block, playing or not, so live keys and effect tails keep sounding while stopped.
- Removing a processor is a hard switch, no crossfade.
- The graph has no cycles. Which connection of a cycle is left out is fixed, so the same one goes on every open and a sidechain mistake usually mutes only a detector.
- Latency compensation leads instead of delaying: each processor sees the transport ahead by the latency after it. Rejected: delaying, which also delays a keyboard played into a track.
- A side input (such as a sidechain) is skipped by the lead, so its source is not led by another track's chain. The detector hears it a little early or late instead.
- Every render is deterministic: the same project gives the same bytes, and anything random takes a seed.

## Templates

`templates/`, `crates/runtime/src/templates.rs`.

- A project keeps its own copy of the template's code. Rejected: the template loaded from the app, which makes a project look different under two releases and needs an "eject" to change one file; hashes of the files, which leave a merge nothing to merge from.
- Composer draws the DAW's song as a score. Quarter notes are equally wide, so a tempo moves no music; adding or removing a bar moves what follows. Rejected: records of its own; seconds across.
- A saved template is laid out as one of `templates/`, so it can be shared as it is. "Tools only" keeps the records at the top and the children their tools require by name (a song's master, no track), and no sections or later tempo: the core cannot tell a tool's setup from its music, but a tool says which children it must have. A project made from one keeps no template code: nothing newer to update from.

## Audio files and the Sampler

`crates/media`, `extensions/sampler`.

- Audio files a record names are held in memory, shared, and dropped when nothing names them. Not streamed: the audio thread never waits for a disk, seeks are instant, renders repeat to the byte.
- Library samples are memory-mapped and read ahead off the audio thread. Rejected: our own disk streaming with ring buffers per voice, far more code for the same result on an SSD.
- Library instruments download only when the composer asks, never for an agent's record: a guess must not cost hundreds of megabytes.

## Export

`crates/export`, `--render`.

- The engine renders at the chosen rate, 44.1 or 48 kHz, as on a device of that rate. Rejected: resampling a 48 kHz render, code for what the engine already gives.
- Loudness is gain alone, from a measure of the whole render, which waits in a temporary file. Its true peak stays 1 dB under full scale before encoding, the room an MP3 needs. Rejected: a limiter, which changes the sound; rendering twice, which a plugin may not repeat.
- MP3 comes from a pure Rust encoder, pinned. Rejected: LAME (LGPL, C). No FLAC yet: the pure Rust encoder holds the whole file in memory.

## Recording

- MIDI input and the computer keys have no tool and no record. Rejected: a tool of the project that plays from the keys, which the interface could delay.
- A recorded note is saved at the tick the engine sounded it, so playback matches what was heard. The raw performance is kept as an asset.
- Live input and the sound of other apps are `project.json` connections. Input and output keep their own clocks: the engine drops old frames or plays silence instead of resampling. Rejected for app sound: a virtual device of our own, which needs a driver install.
- A take goes to the `onTake` of its recorder's parent: what it becomes, and where, is structure, which the project owns. Rejected: the recorder writing children itself.
- A hand writes automation only into a lane that exists, while Write automation is on and the piece plays (touch): the lane lets go while the hand holds its number, and the drag is one undo step that leaves the device's record as it was. It writes few points on a 1/32 grid, so people and agents can read and edit the lane. Rejected: a trip through Bun; recording on the audio thread, which would give the core lanes; latch and write modes; fitting curves to the hand.

## Built-in instruments and effects

One extension per device, one tool with no children. `extensions/filter` is the reference.

- The record is the processor's update, in units an agent can reason about (Hz, dB, seconds, 0 to 1), each number one `Parameter`.
- Every change glides (about 20 ms), including choices, so no edit clicks.
- Each effect has an exact response function, and tests hold the measured sound to it.
- A time that follows the tempo saves the note, not the seconds, and follows a tempo change while playing.
- A latency is a choice, not a knob: one moving with a drag would make every track jump.
- The Drum pad's default kit is not written into records, so changing the kit changes existing projects. Treat it like a change to a saved format.

## Plugin hosting

`extensions/plugin-host`.

- What a plugin sends out is dropped or read into fixed-size queues, so a plugin cannot make our audio thread allocate. What its own process sends back is read only as numbers held to a bound.
- Inspect loads no plugin: printing a project must not run third-party code.
- A session that plays runs every plugin in a process of its own, so a crash or a hang costs its slot: silent (an effect lets the sound through), a problem, back only by Start again. Rejected: one process for all plugins, where one crash silences all; one per track, which this host cannot know.
- The audio thread waits for each sub-block of a plugin's process, one round trip of about 10 µs. Rejected: playing a block behind, a block of latency per plugin in a chain.
- A child is hung after 10 s without its main thread, longer while it loads or saves: a longer freeze is worse than a lost plugin.
- Renders stay in their own process, so a crash fails the render instead of leaving a plugin out. Windows (the OS) plays plugins in-process for now: Rust has no Unix sockets there. So does a window session on macOS.
- Plugins that talk between their own instances see only the ones in their process.

## The window

`crates/window`, on Rune.

- **Rune, because an agent can check a window.** It draws one headless on every OS (`--snap`), lists its controls by key and label (`--tree`) and acts on them (`--act`, `--script`), so agents and tests use it as a composer would. A control declares its meaning once: role, label, state, actions. Rejected: keeping GPUI (pictures on macOS only, no tree, every frame redrawn); both side by side; running older projects' view code: they play, their views do not.
- **Rune is pre-release**: pinned at one rev with its patches (root `Cargo.toml`), and it takes no pull requests.
- **Only `crates/window` knows the toolkit.** It builds the one binary. Every other crate is plain Rust with plain tests.
- **Four layers for what a project draws**, each built only from those below: (1) theme tokens, `extensions/theme.json`; (2) primitives, behaviours and live values, `sdk/view.ts` and the window; (3) components, `extensions/ui/`; (4) blocks, `extensions/ui/blocks/`. Only layer 2 is the SDK. Growth goes to 3 and 4, code the project owns. A new primitive or prop needs a reason no layer above can meet; a more general primitive beats a new one. Rejected: components in the SDK, which grew to 329 exports an agent had to read and no project could change.
- **The window owns what moves; code hears the end.** A camera (`plane`), a hand (`drag`) and a source (`feed`), each named by a key, move each frame. Code reads them as live values and hears a press, a drag's end, a drop or a commit. A view runs again only when meaning changes (records, memory, facts, clock flags, keymap), never for scroll, zoom, a drag's moves, the playhead, meters or hover. Rejected: `onMove`, every pointer move through Bun.
- **A drag plays before it saves**: its `plays` reach the engine unsaved; the `update` at its end is one undo step; Escape puts the sound back. A number an automation lane holds is refused by the host (an `update`) and the window (a drag), unless write automation takes it (Recording), so components need no branches for it.
- **Feeds and facts come from Rust**, registered per tool: display data (peaks, spectra, waves), worked out once a frame and only while shown; and what Rust knows that is no record (fields, a region's length), whose change runs the view.
- **Device cards are project TypeScript** (layer 4): `ui/blocks/cards/<tool>.tsx` when the project has one, else the generic card, built from facts. A card reads its device's feeds and sends it commands. Rejected: cards in Rust, which no project can change.
- **The chrome is Rust on rune-kit**: transport, menus, panels, dialogs. It takes the project's theme tokens, so the two look like one app, and the transport works while code is broken. Rejected: a transport per page.
- **One window.** Small windows became dialogs and Settings a page inside it, so a run without a window acts on them too. Menus are drawn by the app on every OS; file dialogs are the system's. Plugin editors get their own Rune window, the plugin's view a child of it.
- One project a process: "Open project…" quits and starts the app again, which tears down the device, plugins and their windows.
- A `view` is a plain function of its records, run again on every change, with `memo` for parts. Rejected: signals that run a view once, as Solid: a runtime to own, with rules on where a value may be read; Luau, with which an agent did half as well.
- What shows only while the pointer or the keys are on a group is `reveal` in a `hoverGroup`, worked by the window. Rejected: hover and a timer in the view, hidden from the keys.
- Windows installs per user with Inno Setup. Rejected: MSIX, whose sandbox breaks the update, the `PATH` entry and Bun; WiX/MSI, more work.

## Tests

- Logic is tested by plain `#[test]`s where it lives; the window by key and label, on the offline engine with Bun (`crates/window`); the template by `bun test`s beside it. Never by pixel: a label says what a control means, a pixel only where it is.
- Looks are `--snap` pictures an agent judges against DESIGN.md, and rules `--tree` warns of to the view's writer, not the composer: a background flush with one edge of its frame and a few points in from the next, or lying over a border around it. Rejected: golden pictures, which fail on every change and get accepted unseen; switching a rule off.
- Budgets, in `crates/window`: a frame that scrolls, plays, zooms or drags 500 clips or 2,000 notes, or plays the mixer of 64 tracks, costs under 4 ms of the window's work and runs no view. It is the thread's CPU against a fixed piece of work, so a busy machine does not fail it. A run without a window waits for Bun each frame, so it repeats.

## Shortcuts

`crates/typescript/src/shortcuts.rs`.

- **Actions first, keys on top**, as REAPER does: a tool or the app declares actions (name, title, default keys), and one keymap says which keys do each, so every action can be listed, shown and given another key. Rejected: keys only in a tool's `onKey`, which nothing can list or change.
- **The composer's own keys are one file in the support folder**, by action id (`song.record`), for every project: they are the composer's habits, not part of a piece. An agent gives a key by changing an action's defaults in the tool's code, and the composer's keys win. Rejected: a file in the project, which an agent could change under the composer and every project would need again.
- **Actions of one tool may share a key** and are tried in order until one has something to do, so delete takes out notes, else clips. Rejected: a condition per key, a second language beside the code.
- **An action gets no key event**, so anything can run it later: a list, MIDI. `onKey` stays for keys that are held.
- The app's keys win over a tool's on the same key, and tab is no action: the focus can always move on.
- A key an action declares works on every system: `cmd` is ctrl on Windows and Linux, and there is no `ctrl`. The composer's own keys may be any they press.

## The agent sidebar

`crates/agent`.

- The sidebar runs a coding agent CLI: `claude`, or `pi --mode rpc` with an extension of ours for approvals and Pi's sign-in. Rejected: a sidecar with the TypeScript Agent SDK, which adds a runtime and our own protocol; ACP, which adds layers and removes none.
- The composer's own program comes first. Only Claude Code downloads when there is none (215 MB, not bundled); Pi links to its site. A thread keeps its provider.
- Agents are set up on the Settings page, once for the app; the sidebar only links there while none is ready.
- New problems after a turn show to the composer, not the agent, whose docs tell it to read `problems.txt`.
- A picture goes as a picture, any other file as its path; nothing is copied in.
- Versions are named by the agent's own program with its small model, from a capped diff, once the history is still. Without a signed-in agent they stay "Version N"; a name given by hand is never replaced.
- Going back in History tells the agent once, before its next message, in a block the thread does not show. The agent cannot move through history itself yet.

## Tools of the project

`crates/typescript`, `crates/signals`.

- **Written in TypeScript, run by Bun as a separate process**, started fresh on a code change, crash or hang. Rejected: reloading code inside Bun, which cannot read a changed import again nor answer a hang; QuickJS in the app, where an endless loop freezes the window and there is no npm.
- **A tool is a JSON tool of the core**, so its records load and are checked without Bun. **An owner's behaviour runs in Bun and lands as a service.** Rejected: a synchronous call (the window would wait); a structure language read in Rust.
- **The sound is a graph of signals** built with typed SDK functions. Rejected: a text language in between: nobody wrote it, and an eval found it no better.
- **Every record the SDK reaches is described once, by its Rust type** (`schemars`), for the SDK and the agent docs. Rejected: hand-written types, which drift; generating types for a tool of the project, which needs a second load.
- **A built-in device is a child, never part of a sound**: one road to a reverb. Rejected: devices called inside a sound, which cut it into stages and needed their own types, checks and ports.
- **Few concepts:** a tool (record, memory, tick), children, typed ports and connections, signal, view, update, action, clock. What can be a child (`fader`, `lane`, `region`) or a component is one; a new concept needs a reason a child cannot give.
- **No compiled code in a project**, so a tool runs unchanged on any OS. Rejected: Rust a project adds (wasm, dylibs, rebuilding the app), which needs the Xcode tools or MSVC and a build on open.
- **One signals processor**; speed-ups keep renders bit for bit the same. Rejected: compiling to machine code, fast but a compiler to own.
- **A spectrum is a graph per bin** (`spectral`) on the same machine. Rejected: an op per effect, Rust for each new idea; bins as a list to loop over, a second way to write a sound.
- **One import, `./sdk`, over modules by area**, so an agent reads one area. Rejected: `export *`, which makes the internals they share importable.
- **Children reach all code filled with their defaults**, as one shape. A slot names a tool, or `takes(port)`: any tool with that port, so a project's own instruments and plugins fit. Rejected: a list of tools, which grows with every block.
- **A tool's code edits its record and its children's** by the fields it changed, on the records as they are: a late edit takes nothing back.
- **A cable carries sound or events: notes, a number or a bang.** Each end names its kind, so a wrong pair is a type error. A parent's sound is the last stage of its audio output, so a chain is one connection. Rejected: `ownSound`, an end on both sides. A number drives a knob as a lane does, so built-in devices need nothing new.
- **An LFO and an envelope are tools of the DAW template** (`templates/daw/extensions/lfo.ts`): a number cable already moves a knob as a lane does. The LFO follows `beat`, so a render repeats. Rejected: modulation inside the lane, a second road into a number; a block in Rust; moving around the knob's value.
- **Notes in time are a `timeline` child**, launched on the audio thread; its record says where it plays, so a render sounds. Rejected: players a parent declares, a second road for notes; `tick` with `at`, a block late and window-only.
- **A child's notes pass its own feed**, which ends what a removed or moved sender held; a new Bun ends only what the old one held. Rejected: a reset in the core, for every instrument to declare; releasing the whole child, which cuts its clips at every save.
- **A dropped MIDI file becomes timelines, one per part** (a channel of a track), and is not kept: the records hold all it plays. Rejected: a copy in `assets/` that nothing reads; one merged timeline, which plays the drums of a song on its piano.
- **Regions into one place play through one player of their parent.** Rejected: a player per region, which dips at a join and sums an overlap.
- **A tool sends and hears the note events a cable carries**, its `notes` to the sample, whatever plays them. Rejected: tapping the MIDI device thread, deaf to clips and cables.
- **The app ships Bun** at a pinned version, so a composer installs nothing; the agent gets it first on its `PATH`.
