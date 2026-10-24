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
- The agent works in the project folder: Claude Code in the sidebar, or any coding agent in a terminal. It edits files and the runtime applies them live; there is no edit API for agents.
- Every extension is compiled into the one binary. `project.json` enables extensions by name.
- Rust for the core and extensions, GPUI for the window: a one-line extension edit reaches a new running window in about two seconds, and an agent wrote working GPUI views from our docs on the first try. Audio and saved data never depend on GPUI.
- The project, the views and every plugin handle share the main thread, which plugins require. Rejected: a separate control thread; it would split the project from its plugins, and every view would need a synced copy.
- macOS is the main platform. Linux and Windows build and pass the tests. Plugin windows are not offered on Linux, where plugins need an X11 parent and a host run loop.
- System programs come from the system, never the `PATH` (`sound_core::process`).
- Running Sound Tools as a plugin inside another DAW is out of scope.

## The core knows no music

This is the main rule of the codebase.

- The core has no track, clip, note, effect, plugin or audio file type, and depends on no bundled crate. `tooling/workspace-rules` checks this.
- Three layers: the core knows no music; extensions reach it only through core hooks; a project's own tools compose them. The app knows no extension but where it registers them.
- Rust builds blocks, TypeScript owns structure. A block composes through ports and names no DAW idea. Tracks, clips, a mixer layout, a transport bar and what play or record mean are tools of the project, so a project can change them.
- Extensions never depend on each other. What two extensions both need lives in a contract crate, such as `sound-notes` and `sound-media`.
- Tools find each other by port names, not by type: an instrument has an input `notes` and an output `audio`, an effect an input and an output `audio`. So a parent depends on no instrument, effect or plugin.
- The core does own the musical clock, and nothing more musical: position, tempo map, play, pause, jump and a repeat range. Nearly every tool and request talks in bars and beats, so one clock beats one per extension. Sections (named bar ranges) live next to it in `project.json` for the same reason, not in a tool. Which key plays is a tool's choice; the app keeps only app and file shortcuts.

## Tools, instances and behaviours

`crates/core/src/project`.

- A tool is its saved state type. There is no separate tool handle to mix up. Its records load only when its extension is enabled.
- A behaviour is declarative: it runs for every valid state from every source (file, window, undo, load) and declares everything each time. The core sends only the difference, so a behaviour cannot leak processors, and a processor declared again keeps its phase and voices.
- A parent's behaviour reads the state of its children and runs again when anything below it changes.
- One edit group is one engine batch: if a behaviour or the graph refuses, nothing of the group applies. On open, an instance whose behaviour fails is left out and reported instead, so one bad file does not block a project.
- Refusing is for invalid state. State that is valid but cannot be live now (a missing plugin or file) is a problem: that slot is silent and the rest plays.
- Ownership is the folder tree. References are saved ids that keep nothing alive. A reference may arrive before its target; data of a missing extension stays intact.
- A service outside the project that can now do more (a plugin scan, a background render, a file arriving under `assets/`) makes the core run a behaviour again with the record it has: no write, no undo step. `runtime::tick` is the one place that asks these services, so the window, headless, a render and the tests behave alike.
- A tool may say where its instances live (`Place`). A record in the wrong place is a problem that says where it belongs, so an agent's mistake is a message, not silence.
- A tool may register an end, so the core knows the length of the piece without knowing tracks.

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
- A tool decides its form for good: always a folder, or always one file. A path that changes under an agent leads to ignored writes and deleted records coming back.
- The format is chosen for agents and for git: adding a part is one new file, moving a clip to another track is moving a file, and stable JSON with one item per line in long lists makes a small edit a small diff. There is no instance index.
- No schema versions and no migration framework. Old records must load unchanged: a new field is optional with a default that sounds as before, and the runtime does not rewrite a file that did not change.
- An invalid `project.json` stops the project from opening. Anything else that does not load is a problem, and the rest opens. A change of `extensions` applies on reopen.
- What belongs to one machine stays out of the folder (plugin scan cache, window positions, audio devices, agent threads and settings), so a project in git does not change because it was opened on another laptop.
- Assets are never written over, never deleted by the runtime and never part of undo, so no recording or imported file can be lost by an edit. Asset names are checked so a record cannot point outside the project.
- Files are written to a temporary file and renamed into place. No `fsync`: it made undo of a large delete block for most of a second. Power loss is left to git.
- One runtime per project holds `.sound-tools.lock`. Inspect and render open read-only without the lock, so they work next to a running window.

## The live folder and editing

`crates/core/src/project`.

- There is one state application. Window edits, file changes, loading, undo, redo and cancel all apply a group of changes, whole or not at all. Loading is applying from empty.
- The watcher only says which paths changed; the runtime reads them again and applies the difference, so edits, new files, deletes and moves take one road. The runtime groups raw events itself, because a debouncer can split a burst.
- Last write wins everywhere: file edits, open drags, undo, redo, cancel. No field merge and no conflict rejection. Do not reopen this as a sync problem.
- An edit is `begin`, `publish` (live, writes nothing), then `finish` (writes, one undo step) or `cancel`. No undo step ever holds the middle of a gesture, even when a file edit lands during a drag.
- An agent request is one undo step, named after it, however far apart its writes are. A window edit, undo or redo during a request ends that step; later writes make another. Rejected: adding them to the older step, under the composer's edit: undo would go out of order.
- With no request open (an agent in a terminal), outside changes within 15 s of each other, with nothing between, are one undo step; else one request became several. A tool's edits of one label under 1 s apart are one too, written when they stop or after 5 s; undo and other edits end them first, never mid-drag.
- Undo history lives only in the session. The folder is the truth; versions come from git.
- `problems.txt` exists only while a runtime holds the project, so an agent can tell "all is well" from "nobody is watching".

## Agent context

`crates/core/src/project/generated.rs`.

- A map, `AGENTS.md`, plus one file per doc in `agent-docs/`, because one big doc stopped scaling; agents read only the docs their task needs. Rejected: Claude Code skills, no better in an eval and needing settings agents run without.
- A doc never outlives its tools and holds nothing of the machine, nor what an agent reads in the SDK, a field table or a problem.
- Writing tools is an entry doc and deeper docs it names. Rejected: docs by area alone: nearly every tool crosses areas.
- The composer's instructions are plain text: `instructions.md` in the project travels with the piece; one file in the support folder is for every project. Both are edited in the sidebar, so a composer needs no text editor.
- The sidebar runs Claude Code with no settings source and no MCP servers, so a settings file the agent writes in the project gives it no more access. The project's `CLAUDE.md` is loaded back in on purpose.

## Audio engine, transport and time

The threads and the audio thread rules are in [ENGINEERING.md](ENGINEERING.md). `crates/core/src` (`engine.rs`, `processor.rs`, `clock.rs`, `transport.rs`).

- The audio thread plays at real-time priority; on Linux through PipeWire, because cpal raises no thread behind ALSA's PipeWire plugin. Without it a slow laptop played a piece at 2.5 times the cost and dropped out.
- Every audio port is stereo: no channel negotiation, a cable cannot be half connected, and a plugin's stereo output fits one port.
- Time is integers: frames for the engine, ticks for music. Saved positions are ticks or bars, never floats or seconds.
- Time signatures are runs of whole bars and sections start at a bar number, so neither falls inside a bar. Rejected: ticks.
- Changing the time signatures moves bar lines and sections, never notes. Keeping bar positions would move every clip, which goes wrong for clips off the bar lines.
- Bar math lives in `TimeSignatures`, so no extension assumes a fixed bar length.
- The core holds no scheduled events: timeline processors make theirs from the transport, so a seek has nothing to invalidate.
- The loop, and a jump at a tick, go back on the frame of their end, and each processor ahead of the device goes back on its own frame first, so processors need no loop code. Rejected: a seek when the device gets there, which with latency (any filter or saturator) cut what rang one latency early: a click on every repeat.
- The engine runs every processor every block, playing or not, so live keys and effect tails keep sounding while stopped.
- A routing edit keeps the state of surviving processors. Removing a processor is a hard switch, no crossfade.
- The graph has no cycles. A connection that closes one is left out and reported, and the rest plays. Which one is left out is fixed, so the same one goes on every open and a sidechain mistake usually mutes only a detector.
- Latency compensation leads instead of delaying: each processor sees the transport ahead by the latency after it. Delaying would also delay a keyboard played into a track; with leading it waits only for that track's chain.
- A side input (such as a sidechain) is skipped by the lead, so its source is not led by another track's chain. The detector hears it a little early or late instead.
- An output feeding paths of different latency (a send) is delayed on faster ones: at most a second at 48 kHz. Rejected: no maximum, which would allocate on the audio thread.
- Levels leave the audio thread through `Peaks`, sound through a `Scope`. Views measure on their own thread. Rejected: measuring on the audio thread, an FFT per card in every block.
- Every render is deterministic: the same project gives the same bytes. Anything that looks random takes a seed. The click is attached only by the window.

## Templates

`templates/`, `crates/runtime/src/templates.rs`.

- A new project starts from a template: a folder of `templates/` copied into it, with what its `template.json` says of it. A new template is a new folder; `build.rs` embeds them. Bare is an empty project.
- A project keeps the template's code as it wrote it, with its name and release. A later release updates each `extensions/` file the project never changed; for one it changed, the agent gets the old, the new and the project's to merge. Records and `project.json` are the project's from the start. Rejected: the template loaded from the app, which updates by itself but makes a project look different under two releases and needs an "eject" to change one file; hashes of the files, which leave a merge nothing to merge from.
- The DAW is a template of tools of the project, written with the SDK: a song, a track, a mixer and a transport are a project's choice, so they can be changed like any tool. Rejected: keeping the Rust arrangement next to it, two DAWs to maintain.

## Audio files and the Sampler

`crates/media`, `extensions/sampler`.

- Audio files a record names are held in memory, shared, and dropped when nothing names them. Not streamed: the audio thread never waits for a disk, seeks are instant, renders repeat to the byte.
- Our own WAV and AIFF reader, because these formats are a few chunks around plain samples.
- The Sampler reads the part of SFZ real libraries need and ignores the rest: a third-party file that plays a little plainer beats one that is refused.
- Library samples are memory-mapped and read ahead off the audio thread. Only files nothing writes in place are mapped, because a mapped file cut short ends the process. Rejected: our own disk streaming with ring buffers per voice, far more code for the same result on an SSD.
- The library is a catalog in code of free instruments at pinned commits, so an id always plays the same. Downloads are shared machine data. Nothing downloads by itself, not for an agent either: its guess must not spend hundreds of megabytes.
- In the window an instrument loads in the background and the old one plays until it is ready. A render, an inspect and the tests load at once, so they play what the records say from the first block.
- Other sample rates go through a stateless resampler, so any block can render on its own from any place. Rejected: resamplers that keep filter state.
- The drawing thread never reads a file; waveform overviews are made in the background and never saved.

## Recording

- MIDI input and the computer keys have no tool and no record. Both reach the audio thread through one lock-free ring, so nothing the interface does can delay sound. Rejected: a tool of the project that plays from the keys.
- A recorded note is saved at the tick the engine sounded it, so playback matches what was heard. The raw performance is also saved as an asset, written before the timeline, so it survives it. Its times are microseconds, because a tick means nothing without a tempo map.
- Audio recording does not pass through the engine. A take is placed by mapping its capture time to the engine frame sounding then, which works with any latency compensation.
- Live input and the sound of other apps are `project.json` connections. Input and output keep their own clocks: the engine drops old frames or plays silence instead of resampling, so the delay stays bounded. The window opens an input only while something needs it, so a project without one never asks for the microphone. Rejected for app sound: a virtual device of our own, which needs a driver install.
- The audio devices are chosen per machine, never in a project. Picking one restarts the app, because the engine runs at the rate of its output device.
- A tempo change ends a take, because the take was heard under the old tempo.
- A `recorder` writes a take's files and hands its record to the parent's `onTake`: what a take becomes, and where, is structure, which the project owns. Rejected: the recorder writing children itself.

## Built-in instruments and effects

One extension per device, one tool with no children. `extensions/filter` is the reference.

- The record is the processor's update, in units an agent can reason about (Hz, dB, seconds, 0 to 1). Every number is one `Parameter`: validation, knobs, reset, automation and doc tests all read it.
- Every change glides (about 20 ms), including choices, so no edit clicks.
- Each effect has an exact response function, and tests hold the measured sound to it.
- A time that follows the tempo saves the note, not the seconds, and follows a tempo change while playing.
- A latency is a choice, not a knob: one moving with a drag would make every track jump. The Filter always reports its 4x latency, so drive automated across 0 stays in phase with the dry mix. Instruments report none; a bent voice is 1.3 ms late.
- The Drum pad's default kit is not written into records, so changing the kit changes existing projects. Treat it like a change to a saved format.
- The metronome is not project state: a tool would mean a file in `state/`, and turning the click on would rewrite a musical record.

## Plugin hosting

`extensions/plugin-host`. The core knows no plugins.

- One `plugin` tool for every slot and both formats. How its parent wires it decides instrument or effect; what the plugin says only decides which picker offers it, because plugins mislabel themselves.
- Nothing a plugin sends out is read, so a plugin cannot make our audio thread allocate.
- A pin is a parameter the record holds; the rest stays in the state asset. The record wins over the state. Automation of a pin is never an undo step.
- Scanning runs plugin code, so it runs in child processes, one per bundle with a deadline. A bundle that crashed or hung is not tried again until the composer asks. The window never waits for a scan.
- Plugin state is not project state: never an undo step, and unchanged bytes are not written again, so a session that changed nothing leaves no diff.
- A slot with no plugin passes its input through: a missing effect lets the sound through, a missing instrument is silent.
- Inspect loads no plugin. Printing a project must not run third-party code.
- A parameter value stays in the format's own units, so what is read from a plugin can be sent back unchanged.
- VST 3 goes through our own safe layer in `src/vst3/`. Bundles are never unloaded: unloading runs static destructors while views and threads may still exist.
- The host owns every plugin window; the floating option is not supported by real plugins.
- Write "VST 3" in plain text, with Steinberg's attribution line where a VST plugin is offered.

## The window

`crates/ui` and `crates/runtime/src/window`.

- Views read state when they render and keep no copy of saved state, so an outside edit shows at once, also mid-drag.
- Views and device offers are registered per tool, so a rack knows no instrument or plugin.
- The playhead is its own entity, because GPUI redraws a notified view and everything above it.
- `crates/ui` holds the design system and general components; musical interfaces such as a piano roll belong to a project's tools.
- Extensions, fonts, icons and agent docs are compiled in. "Open project…" starts a new process, because that tears down the device, plugins and their windows for free.
- The app updates itself from the latest GitHub release, with the system's curl and a checked sha256, at the next start: GPUI gives a quit too little time to copy an app. A failed update is not downloaded again.
- Windows installs per user with an Inno Setup program the update runs silently. It moves a running program aside, so no window closes. Rejected: MSIX, whose sandbox breaks the update, the `PATH` entry and Bun; WiX/MSI, more work.
- No error tracking in the cloud. The window's stdout and stderr go to `log.txt` in the support folder, which the composer can send.

## The agent sidebar

`crates/agent`.

- The sidebar runs Claude Code as a child process in the project folder, editing files like a terminal agent. Rejected: a sidecar with the TypeScript Agent SDK, which adds a runtime and our own protocol. Rejected for now: ACP, which adds layers and removes none; it is the way to add more agents later.
- The window has a generic left panel slot and names no agent type.
- Each message is one request: one undo step.
- The thread state is built from provider-neutral events and holds no process, so tests feed it events.
- Threads, settings and whether the panel is open are kept per machine, never in the project, so an agent cannot change the interface or raise its own access by editing a file.
- The provider keeps the conversation; the app keeps only its session id and a log to show it.
- The app runs the composer's own `claude`, else **Set up** downloads a pinned, checked version. Rejected: bundling `claude`: 215 MB more per download, and an app release per version.
- Sign-in is Claude Code's own. The app never reads or keeps a credential.
- New problems after a turn are shown to the composer. Nothing goes back to the agent; its docs tell it to read `problems.txt`.

## Tools of the project

A tool an agent writes plays at once, in the installed app, with no build. `crates/typescript`, `crates/signals`.

- **Written in TypeScript, run by Bun as a separate process.** No tool, no Bun; a code change, crash or hang starts a fresh one, without code that hung until the folder changes. Rejected: reloading code inside Bun, which cannot read a changed import again nor answer a hang; an engine in the app (QuickJS), where an endless loop freezes the window and there is no npm.
- **A tool is a JSON tool of the core.** Its records are checked in Rust, so a record loads and an agent hears what is wrong without Bun.
- **An owner's behaviour runs in Bun and lands as a service.** Until Bun answers a change, the last declaration plays. Renders and inspect wait. Rejected: a synchronous call (the window would wait); a structure language read in Rust.
- **The sound is a graph of signals** built with typed SDK functions. Rejected: a text language in between: nobody wrote it, and an eval found it no better.
- **Every record the SDK reaches is described once, by its Rust type** (`schemars`), for the SDK and the agent docs. Rejected: hand-written types, which drift; generating types for a tool of the project, which needs a second load.
- **A built-in device is a child, never part of a sound.** One road to a reverb. Rejected: devices called inside a sound, which cut it into stages: a second road that needed its own types, checks and engine ports.
- **Fast things never cross to Bun.** Knobs, choices, live controls and triggers change the sound without Bun, which sends a graph for every combination of choices up front. On screen, live values (`now`) are worked out natively every frame; code hears a drag once, at its end. Rejected: `onMove`, every pointer move through Bun.
- **Few concepts:** a tool (record, memory, tick), children, typed ports and connections, signal, view, update, clock. What can be a child or a helper is one: mixing is a `fader`, automation a `lane`, an audio clip a `region`, the transport bar a component. A new concept needs a reason a child cannot give.
- **No compiled code in a project.** Rejected: Rust a project adds (rebuilding the app, wasm, dylibs), which needs the Xcode tools or MSVC and a build when a project opens. A TS tool runs unchanged on any OS.
- **One signals processor**, made fast by working out once what does not change over a span. Speed-ups keep renders bit for bit the same. Rejected: compiling to machine code, fast but a compiler to own.
- **A spectrum is a graph per bin** (`spectral`) on the same machine, each bin with its own memory. Rejected: an op per effect, Rust for each new idea; bins as a list to loop over, a second way to write a sound.
- **A view runs once, as in Solid.** What changes is a function the window runs again when what it read changes; only the ops of what changed cross, onto a node table the window keeps by id. Our own runtime in the SDK, so misuse is an error that says the fix. Rejected: a whole tree per change, as React (1 MB a click at 20,000 notes); Solid itself, a Babel transform in every project and DOM only.
- **One import, `./sdk`, over modules by area**, so an agent reads one area. Rejected: `export *`, which makes the internals they share importable.
- **Children reach all code filled with their defaults**, as one shape. A slot names a tool, or `takes(port)`: any tool with that port, so a project's own instruments and plugins fit. Rejected: a list of tools, which grows with every block.
- **A tool's code edits its record and its children's** by the fields it changed, on the records as they are: a late edit takes nothing back.
- **A cable carries sound or events: notes, a number or a bang**, all fixed-size. Each end names its kind, so a wrong pair is a type error. A parent's sound is the last stage of its audio output, so a chain is one connection. Rejected: `ownSound`, an end on both sides. A number drives a knob as a lane does, so built-in devices need nothing new. An instrument sends no notes or bangs: voices come and go.
- **Notes in time are a `timeline` child**: once at a song position, or looping, with launches quantized on the audio thread. Its record says where it plays, so a render sounds. Rejected: players a parent declares, a second road for notes; `tick` with `at`, a block late and window-only.
- **A child's notes pass its own feed**, which releases the child when its senders change (a removed or moved sender sends no off there) and holds a pitch until its last sender lets go. The code of a tool is one sender: a new Bun ends only what the old one held. Rejected: a reset in the core, for every instrument to declare; releasing the whole child, which cuts its clips at every save.
- **Regions into one place play through one player of their parent**: a crossfade where they meet, the last by name where they overlap. Rejected: a player per region, which dips at a join and sums an overlap.
- **A tool sends and hears the note events a cable carries**, its `notes` to the sample, whatever plays them. Rejected: tapping the MIDI device thread, deaf to clips and cables.
- **What the tools print goes to `extensions/log.txt` too**, its last lines since Bun started, only from a runtime that holds the project. It outlives a close.
- **The app ships Bun** at a pinned version, so a composer installs nothing. The agent gets the same Bun first on its `PATH`.
