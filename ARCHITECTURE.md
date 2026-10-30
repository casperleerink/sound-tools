# Technical architecture

The technical decisions, why we made them, and what we rejected. How something works is in the code; each section names where. A line belongs here only if the code cannot say it.

## Terms

| Term | Meaning |
| --- | --- |
| Extension | A Rust crate that registers tools, views, device offers and agent docs through the public SDK. Bundled extensions use the same SDK a user extension would. |
| Plugin | A third-party audio plugin (CLAP, VST 3). Never our own extensions. |
| Tool | A composer-facing capability an extension defines. In code, a tool is its saved state type. |
| Tool of the project | A tool a project defines in TypeScript under `extensions/`, with no build. |
| Block | A tool written in Rust that tools of the project use as a child: `timeline`, `fader`, a reverb, the plugin host. |
| Template | The files a new project starts from, a folder of `templates/`. |
| Instance | One use of a tool in a project, with its own record. |
| Record | The saved JSON state of one instance, one file in `state/`. |
| Behaviour | A function from the state of an instance to what it needs in the engine: processors, updates, connections, ports. |
| Asset | Bytes under `assets/` the core never looks inside: audio files, raw takes, plugin states. |
| Problem | A part of the project that is not live, listed in `problems.txt`. Never a reason to refuse the rest. |

## Shape of the system

- One process, the runtime (`crates/runtime`), opens one project folder: as a window, headless, or read-only to inspect, render or analyze.
- Agents only edit files in the project folder, and the runtime applies them live. There is no edit API for agents.
- Every extension is compiled into the one binary. `project.json` enables extensions by name.
- Rust for the core and extensions, GPUI for the window: a one-line extension edit reaches a new running window in about two seconds, and an agent wrote working GPUI views from our docs on the first try. Audio and saved data never depend on GPUI.
- The project, the views and every plugin handle share the main thread, which plugins require. Rejected: a separate control thread; it would split the project from its plugins, and every view would need a synced copy.
- macOS is the main platform. No plugin windows on Linux: plugins there need an X11 parent and a host run loop.
- System programs come from the system, never the `PATH` (`sound_core::process`).
- Running Sound Tools as a plugin inside another DAW is out of scope.

## The core knows no music

This is the main rule of the codebase.

- The core has no track, clip, note, effect, plugin or audio file type, and depends on no other workspace crate, which `tooling/workspace-rules` checks.
- Three layers: the core knows no music; extensions build blocks in Rust through core hooks, and a block names no DAW idea; tools of the project, in TypeScript, own the structure (tracks, clips, a mixer layout, a transport bar, what play and record mean), so a project can change them. The app knows no extension but where it registers them.
- Tools find each other by port names, not by type, so a parent depends on no instrument, effect or plugin.
- The core does own the musical clock, and nothing more musical: position, tempo map, play, pause, jump and a repeat range. Nearly every tool and request talks in bars and beats, so one clock beats one per extension. Sections (named bar ranges) live next to it in `project.json` for the same reason, not in a tool. Which key plays is a tool's choice; the app keeps only app and file shortcuts.

## Tools, instances and behaviours

`crates/core/src/project`.

- A behaviour is declarative: it runs for every valid state from every source and declares everything; the core sends only the difference, so it cannot leak processors.
- A parent's behaviour reads the state of its children and runs again when anything below it changes.
- Refusing is for invalid state. State that is valid but cannot be live now (a missing plugin or file) is a problem, and the rest plays.
- References are saved ids that keep nothing alive. A reference may arrive before its target; data of a missing extension stays intact.
- A service outside the project that can now do more (a plugin scan, a background render, a file arriving under `assets/`) makes the core run a behaviour again with the record it has: no write, no undo step (`runtime::tick`).

## Project storage

A project is a folder of JSON records and assets. Together with its extensions it is the whole piece. `crates/core/src/project/storage.rs`.

```text
my-piece/
  project.json              format, extensions, tempo map, sections, hand-made connections
  state/
    song/                   a tool that owns children is a folder
      instance.json
      piano/
        instance.json
        synth.json          a tool that owns no children is one file
        verse-a.json
  assets/                   opaque files: audio/, takes/, plugin-state/
  AGENTS.md  CLAUDE.md  agent-docs/  problems.txt    written by the runtime
  instructions.md           the composer's instructions to agents, never written by the runtime
```

- The folder tree is the ownership tree, so one parent per child and no cycles come for free. The id of an instance is its path; display names live in the record, so a rename never moves a file.
- A tool decides its form (folder or file) for good: a path that changes under an agent leads to ignored writes and deleted records coming back.
- The format is chosen for agents and for git: adding a part is one new file, moving a clip to another track is moving a file. There is no instance index.
- No schema versions and no migration framework. Old records must load unchanged: a new field is optional with a default that sounds as before, and the runtime does not rewrite a file that did not change.
- Only an invalid `project.json` stops a project from opening. A change of `extensions` applies on reopen.
- What belongs to one machine stays out of the folder (plugin scan cache, window positions, audio devices, agent threads and settings), so a project in git does not change because it was opened on another laptop.
- Recordings and imported files are never written over, never deleted by the runtime and never part of undo, so an edit cannot lose one.
- Files are written to a temporary file and renamed into place. No `fsync`: it made undo of a large delete block for most of a second. Power loss is left to git.
- One runtime per project holds `.sound-tools.lock`. Inspect and render open read-only without the lock, so they work next to a running window.

## The live folder and editing

`crates/core/src/project`.

- There is one state application. Window edits, file changes, loading, undo, redo and cancel all apply a group of changes, whole or not at all. Loading is applying from empty.
- The watcher only says which paths changed; the runtime reads them again and applies the difference, so edits, new files, deletes and moves take one road. The runtime groups raw events itself, because a debouncer can split a burst.
- Last write wins everywhere: file edits, open drags, undo, redo, cancel. No field merge and no conflict rejection. Do not reopen this as a sync problem.
- An agent request is one undo step (`Project::begin_request`). Rejected: adding writes that come after a composer's edit to the older step: undo would go out of order.
- Undo history lives only in the session. The folder is the truth; versions come from git.

## Agent context

`crates/core/src/project/generated.rs`.

- One map, `AGENTS.md`, and one file per doc in `agent-docs/`, because one big doc stopped scaling. Rejected: Claude Code skills, no better in an eval and needing settings agents run without.
- A doc never outlives its tools and holds nothing of the machine, nor what an agent reads in the SDK, a field table or a problem.
- Writing tools is an entry doc and deeper docs it names. Rejected: docs by area alone: nearly every tool crosses areas.
- The composer's instructions are plain text edited in the sidebar: `instructions.md` travels with the project; one in the support folder covers every project.

## Audio engine, transport and time

The threads and the audio thread rules are in [ENGINEERING.md](ENGINEERING.md). `crates/core/src` (`engine.rs`, `processor.rs`, `clock.rs`, `transport.rs`).

- The audio thread plays at real-time priority, also on Linux. Without it a slow laptop played a piece at 2.5 times the cost and dropped out.
- Every audio port is stereo: no channel negotiation, a cable cannot be half connected, and a plugin's stereo output fits one port.
- Time signatures are runs of whole bars and sections start at a bar number, so neither falls inside a bar. Rejected: ticks.
- Changing the time signatures moves bar lines and sections, never notes. Keeping bar positions would move every clip, which goes wrong for clips off the bar lines.
- Bar math lives in `TimeSignatures`, so no extension assumes a fixed bar length.
- The loop, and a jump at a tick, go back on the frame of their end, and each processor ahead of the device goes back on its own frame first, so processors need no loop code. Rejected: a seek when the device gets there, which with latency (any filter or saturator) cut what rang one latency early: a click on every repeat.
- The engine runs every processor every block, playing or not, so live keys and effect tails keep sounding while stopped.
- Removing a processor is a hard switch, no crossfade.
- The graph has no cycles. Which connection of a cycle is left out is fixed, so the same one goes on every open and a sidechain mistake usually mutes only a detector.
- Latency compensation leads instead of delaying: each processor sees the transport ahead by the latency after it. Delaying would also delay a keyboard played into a track; with leading it waits only for that track's chain.
- A side input (such as a sidechain) is skipped by the lead, so its source is not led by another track's chain. The detector hears it a little early or late instead.
- Every render is deterministic: the same project gives the same bytes. Anything that looks random takes a seed.

## Templates

`templates/`, `crates/runtime/src/templates.rs`.

- A project keeps its own copy of the template's code. Rejected: the template loaded from the app, which updates by itself but makes a project look different under two releases and needs an "eject" to change one file; hashes of the files, which leave a merge nothing to merge from.
- The DAW is only a template of tools of the project. Rejected: keeping the Rust arrangement next to it, two DAWs to maintain.

## Audio files and the Sampler

`crates/media`, `extensions/sampler`.

- Audio files a record names are held in memory, shared, and dropped when nothing names them. Not streamed: the audio thread never waits for a disk, seeks are instant, renders repeat to the byte.
- Library samples are memory-mapped and read ahead off the audio thread. Rejected: our own disk streaming with ring buffers per voice, far more code for the same result on an SSD.
- Library instruments download only when the composer asks, never for an agent's record: its guess must not spend hundreds of megabytes.

## Recording

- MIDI input and the computer keys have no tool and no record. Rejected: a tool of the project that plays from the keys, which the interface could delay.
- A recorded note is saved at the tick the engine sounded it, so playback matches what was heard. The raw performance is kept as an asset.
- Live input and the sound of other apps are `project.json` connections. Input and output keep their own clocks: the engine drops old frames or plays silence instead of resampling. Rejected for app sound: a virtual device of our own, which needs a driver install.
- A take goes to the `onTake` of its recorder's parent: what a take becomes, and where, is structure, which the project owns. Rejected: the recorder writing children itself.

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

- Nothing a plugin sends out is read, so a plugin cannot make our audio thread allocate.
- Inspect loads no plugin. Printing a project must not run third-party code.

## The window

`crates/ui` and `crates/runtime/src/window`.

- Musical interfaces such as a piano roll belong to a project's tools, not `crates/ui`.
- "Open project…" starts a new process, because that tears down the device, plugins and their windows for free.
- Windows installs per user with Inno Setup. Rejected: MSIX, whose sandbox breaks the update, the `PATH` entry and Bun; WiX/MSI, more work.

## The agent sidebar

`crates/agent`.

- The sidebar runs the `claude` CLI. Rejected: a sidecar with the TypeScript Agent SDK, which adds a runtime and our own protocol. Rejected for now: ACP, which adds layers and removes none; it is how more agents come later.
- Claude Code is not bundled: 215 MB more per download, and a release per version.
- New problems after a turn show to the composer, not the agent: its docs tell it to read `problems.txt`.
- A picture goes as a picture, any other file as its path; nothing is copied in. Saved threads keep only their names.

## Tools of the project

`crates/typescript`, `crates/signals`.

- **Written in TypeScript, run by Bun as a separate process**, started fresh on a code change, crash or hang. Rejected: reloading code inside Bun, which cannot read a changed import again nor answer a hang; an engine in the app (QuickJS), where an endless loop freezes the window and there is no npm.
- **A tool is a JSON tool of the core**, so its records load and are checked without Bun.
- **An owner's behaviour runs in Bun and lands as a service.** Rejected: a synchronous call (the window would wait); a structure language read in Rust.
- **The sound is a graph of signals** built with typed SDK functions. Rejected: a text language in between: nobody wrote it, and an eval found it no better.
- **Every record the SDK reaches is described once, by its Rust type** (`schemars`), for the SDK and the agent docs. Rejected: hand-written types, which drift; generating types for a tool of the project, which needs a second load.
- **A built-in device is a child, never part of a sound.** One road to a reverb. Rejected: devices called inside a sound, which cut it into stages: a second road that needed its own types, checks and engine ports.
- **Fast things never cross to Bun.** Knobs, choices, live controls, triggers and live values on screen (`now`) work without it; code hears a drag once, at its end. Rejected: `onMove`, every pointer move through Bun.
- **Few concepts:** a tool (record, memory, tick), children, typed ports and connections, signal, view, update, clock. What can be a child (`fader`, `lane`, `region`) or a component is one; a new concept needs a reason a child cannot give.
- **No compiled code in a project.** Rejected: Rust a project adds (rebuilding the app, wasm, dylibs), which needs the Xcode tools or MSVC and a build when a project opens. A TS tool runs unchanged on any OS.
- **One signals processor**; speed-ups keep renders bit for bit the same. Rejected: compiling to machine code, fast but a compiler to own.
- **A spectrum is a graph per bin** (`spectral`) on the same machine. Rejected: an op per effect, Rust for each new idea; bins as a list to loop over, a second way to write a sound.
- **A view runs once, as in Solid**; only the ops of what changed cross to the window. Our own runtime in the SDK, so misuse is an error that says the fix. Rejected: a whole tree per change, as React (1 MB a click at 20,000 notes); Solid itself, a Babel transform in every project and DOM only.
- **One import, `./sdk`, over modules by area**, so an agent reads one area. Rejected: `export *`, which makes the internals they share importable.
- **Children reach all code filled with their defaults**, as one shape. A slot names a tool, or `takes(port)`: any tool with that port, so a project's own instruments and plugins fit. Rejected: a list of tools, which grows with every block.
- **A tool's code edits its record and its children's** by the fields it changed, on the records as they are: a late edit takes nothing back.
- **A cable carries sound or events: notes, a number or a bang.** Each end names its kind, so a wrong pair is a type error. A parent's sound is the last stage of its audio output, so a chain is one connection. Rejected: `ownSound`, an end on both sides. A number drives a knob as a lane does, so built-in devices need nothing new.
- **Notes in time are a `timeline` child**, launched on the audio thread; its record says where it plays, so a render sounds. Rejected: players a parent declares, a second road for notes; `tick` with `at`, a block late and window-only.
- **A child's notes pass its own feed**, which ends what a removed or moved sender held; a new Bun ends only what the old one held. Rejected: a reset in the core, for every instrument to declare; releasing the whole child, which cuts its clips at every save.
- **Regions into one place play through one player of their parent.** Rejected: a player per region, which dips at a join and sums an overlap.
- **A tool sends and hears the note events a cable carries**, its `notes` to the sample, whatever plays them. Rejected: tapping the MIDI device thread, deaf to clips and cables.
- **The app ships Bun** at a pinned version, so a composer installs nothing. The agent gets the same Bun first on its `PATH`.
