# Technical architecture

The technical decisions, why we made them, and what we rejected. How something works is in the code; each section names where. A line belongs here only if the code cannot say it.

## Terms

| Term | Meaning |
| --- | --- |
| Extension | A Rust crate that registers tools, views, device offers and agent docs through the public SDK. Bundled extensions use the same SDK a user extension would. |
| Plugin | A third-party audio plugin (CLAP, VST 3). Never our own extensions. |
| Tool | A composer-facing capability an extension defines. In code, a tool is its saved state type. |
| Tool of the project | A tool a project defines itself in TypeScript, under `extensions/` in its folder, with no build. |
| Instance | One use of a tool in a project, with its own record. |
| Record | The saved JSON state of one instance, one file in `state/`. |
| Behaviour | A function from the state of an instance to what it needs in the engine: processors, updates, connections, ports. |
| Derive | A function from a changed record to more changes in the same edit group. |
| Asset | Bytes under `assets/` the core never looks inside: audio files, raw takes, plugin states. |
| Problem | A part of the project that is not live, listed in `problems.txt`. Never a reason to refuse the rest. |

## Shape of the system

- One process, the runtime (`crates/runtime`), opens one project folder: as a window, headless, or read-only to inspect, render or analyze.
- The agent works in the project folder: Claude Code in the sidebar, or any coding agent in a terminal. It edits files and the runtime applies them live. There is no separate edit API for agents.
- Every extension is compiled into the one binary. `project.json` enables extensions by name.
- Rust for the core and extensions, GPUI for the window: a one-line extension edit reaches a new running window in about two seconds, and an agent wrote working GPUI views from our docs on the first try. Audio and saved data never depend on GPUI.
- macOS is the main platform. Linux and Windows build and pass the tests; platform code sits behind `cfg`. Plugin windows are not offered on Linux, where plugins need an X11 parent and a host run loop.
- System programs come from the system, never the `PATH` (`sound_core::process`).
- Sound Tools is a standalone app. Running it as a plugin inside another DAW is out of scope.

## The core knows no music

This is the main rule of the codebase.

- The core has no track, clip, note, effect, plugin or audio file type, and depends on no bundled crate. `tooling/workspace-rules` checks this.
- Extensions never depend on each other. What two extensions both need lives in a contract crate: `sound-notes` (notes, clips, the realtime note events, port names, voice handling) and `sound-media` (audio files and measuring a sound).
- Tools find each other by port names, not by type. An instrument has an event input `notes` and an audio output `audio`; an effect an audio input and output named `audio`, and maybe a `sidechain` input. So any tool with those ports fits a track slot, and the arrangement depends on no instrument, effect or plugin.
- The core does own the musical clock (tempo map, time signatures, ticks). Nearly every tool and agent request talks in bars and beats, so one clock in the core beats one per extension.
- `extensions/tone` is a small non-musical tool, not in the default project. It proves the core rules hold for a tool shaped differently from the arrangement.

## Tools, instances and behaviours

`crates/core/src/project`.

- A tool is its saved state type. There is no separate tool handle to mix up. Its records load only when its extension is enabled.
- A behaviour is declarative: it runs for every valid state from every source (file, window, undo, load) and declares everything each time. The core sends only the difference. So a behaviour cannot leak processors, and a processor declared again keeps its phase and voices.
- A parent's behaviour reads the typed state of its children and runs again when anything below it changes.
- One edit group is one engine batch: if a behaviour or the graph refuses, nothing of the group applies. On open, an instance whose behaviour fails is left out and reported instead, as is a connection that closes a cycle at any time, so one bad file does not block a project.
- Refusing is for invalid state. State that is valid but cannot be live now (a missing plugin or file) is a problem: that slot is silent and the rest plays.
- Ownership is the folder tree. References are saved ids that keep nothing alive. A reference may arrive before its target; data of a missing extension stays intact.
- A service outside the project that can now do more (a plugin scan, a background render, a file arriving under `assets/`) makes the core run a behaviour again with the record it has: no write, no undo step. `runtime::tick` is the one place that asks these services, so the window, headless, a render and the tests behave alike.
- A tool may say where its instances live (`Place`). A record in the wrong place is a problem that says where it belongs, so an agent's mistake is a message, not silence.
- A derive turns one changed record into more changes in the same group: one engine batch, one undo step. It never runs on load, undo, redo or cancel, because the files already hold its output. It cannot fail: what it cannot compute is a problem. Rejected: running derives on load (cost on every inspect, and it could silently disagree with the files).
- A tool may register a summary and an end, so the core can print "what plays where" and the transport length without knowing tracks.

## Project storage

A project is a folder of JSON records and assets. Together with its extensions it is the whole piece. `crates/core/src/project/storage.rs`.

```text
my-piece/
  project.json              format, enabled extensions, tempo map, hand-made connections
  state/
    arrangement/            a tool that owns children is a folder
      instance.json
      piano/
        instance.json
        instrument.json     a tool that owns no children is one file
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

`crates/core/src/project` (`apply.rs`, `editing.rs`, `history.rs`, `outside.rs`, `watcher.rs`).

- There is one state application. Window edits, file changes, loading, undo, redo and cancel all apply a group of changes, whole or not at all. Loading is applying from empty.
- The watcher only says which paths changed; the runtime reads them again and applies the difference. So edits, new files, deletes and moves take one road. The runtime groups raw events itself, because a debouncer can split a burst.
- Last write wins everywhere: file edits, open drags, undo, redo, cancel. No field merge and no conflict rejection. Do not reopen this as a sync problem.
- An edit is `begin`, `publish` (live, writes nothing), then `finish` (writes, one undo step) or `cancel`. No undo step ever holds the middle of a gesture, even when a file edit lands during a drag.
- An agent request is one undo step named after the request, however far apart its writes are. A window edit, undo or redo during a request ends that step; later writes make a new step. Rejected: adding them to the older step, which lies under the composer's edit, so undo would no longer go back in order.
- With no request open (an agent in a terminal), outside changes within 15 s of each other, with nothing else between, are one undo step. Without this, one request became several undo steps.
- Undo history lives only in the session. The folder is the truth; versions come from git.
- `problems.txt` exists only while a runtime holds the project, so an agent can tell "all is well" from "nobody is watching".

## Agent context

`crates/core/src/project/generated.rs`.

- The runtime writes `AGENTS.md` (a map, with one line per doc on when to open it), `CLAUDE.md` (imports the map), and one file per doc in `agent-docs/`. A map plus docs, because one big doc stopped scaling with the extensions; agents read only the docs their task needs.
- Extensions register their docs; the sources are product content (`extensions/*/agent-doc.md`, `crates/core/src/project/*.md`). A doc never outlives its tools, and the generated docs contain nothing of the machine.
- A runtime test loads every JSON example in every doc, so the docs cannot drift from the formats.
- The composer's instructions are plain text: `instructions.md` in the project travels with the piece; one file in the support folder is for every project. Both are edited in the sidebar, so a composer never needs a text editor.
- The sidebar runs Claude Code with no settings source and no MCP servers, so a settings file the agent writes in the project gives it no more access. The project's `CLAUDE.md` is loaded back in on purpose.

## Audio engine, transport and time

The threads and the audio thread rules are in [ENGINEERING.md](ENGINEERING.md). `crates/core/src` (`engine.rs`, `processor.rs`, `clock.rs`, `transport.rs`).

- The audio thread plays at real-time priority; on Linux through PipeWire, because cpal raises no thread behind ALSA's PipeWire plugin. Without it a slow laptop played the same piece at 2.5 times the cost and dropped out.
- Every audio port is stereo: no channel negotiation, a cable cannot be half connected, and a plugin's stereo output fits one port.
- Time is integers: frames for the engine, ticks (960 per quarter) for music. Saved positions are ticks, never floats or seconds. Tick to frame rounds in one place, so every part agrees.
- Time signatures are runs of whole bars, so a change can only fall on a bar line. Rejected: changes at ticks, which could fall inside a bar.
- Changing the time signatures moves bar lines, never notes. Keeping bar positions would need a derive that moves every clip, and it would go wrong for clips placed off the bar lines.
- Bar math lives in `TimeSignatures`, so no extension assumes a fixed bar length.
- The core holds no scheduled events. Timeline processors make their events block by block from the transport, so a seek has nothing to invalidate and timing never depends on the control thread.
- The loop goes back by a seek on the frame of its end, so processors need no loop code. With latency that leaves a gap as long as the latency. Rejected: wrapping each processor ahead of the device, far more transport code.
- The engine runs every processor every block, playing or not, so live keys and effect tails keep sounding while stopped.
- A routing edit keeps the state of surviving processors. Removing a processor is a hard switch, no crossfade.
- The graph has no cycles. A connection that closes one is left out and reported, and the rest plays. Which one is left out is fixed, so the same one goes on every open and a sidechain mistake usually mutes only a detector.
- Latency compensation leads instead of delaying: each processor sees the transport ahead by the latency after it. No delay lines and no maximum. Delaying after a track would also delay a keyboard played into it; leading keeps live playing at the cost of only that track's own chain.
- A side input (such as a sidechain) is skipped by the lead, so its source is not led by another track's chain. The detector hears it a little early or late instead.
- A send to a bus, when it comes, will need a delay on the shorter path: the one exception to "no delay lines".
- Levels leave the audio thread through `Peaks` (atomics), sound through a `Scope` (a ring of atomics). Views measure on their own thread. Rejected: measuring on the audio thread, an FFT per card in every block.
- Every render is deterministic: the same project gives the same bytes. Nothing random; anything that looks random takes a seed. The click is attached only by the window.

## The arrangement

`extensions/arrangement`.

- It owns tracks, clips, the track mixer and the master. Mixer values are fields of the track record, because they are the track.
- A track's chain is its instrument, then the effects its `effects` list names. One list decides the order, so a reorder is one record and one undo step. Rejected: an order number in each effect record (two files per move, and ties).
- Bypass and sidechain are saved on the track's effect slot, not in the effect's record, so plugins and built-in effects share them. The arrangement wires sidechains, as only it sees every track, and names a loop of keys by its tracks, because the core's cycle message names processors an agent cannot act on.
- Solo mutes the other tracks in their mixers. That is why the mixers are in the arrangement: only the owner of every track sees them all.
- The master limiter is fixed code of the arrangement, not a tool, so old projects get it without a file changing. It is bit-transparent under its ceiling. The core itself limits nothing.
- Every note starts inside its clip, else the record does not load, so a note written with a project position is an error and not silence. No note is ever stuck: an edit, move, delete or tempo change ends what it started.
- Expression lanes of a clip are sent only when they move, so outside every clip a lane is at rest and a seek or stop needs no case of its own.
- Automation lanes are on the track, in project ticks and the number's own units. A lane names a device by its file name and a number by its path in the record; a device declares what it takes in one `AutomationInput`. So the arrangement knows no device. Whole numbers are not automatable, because a lane is a straight line. A lane that does not fit is a problem; the rest plays.
- A lane is a straight line on the knob's travel, so a sweep sounds even and the knob and the lane agree.
- Each automated device gets its own small player on the track, so it sees that device's latency lead and its events need no device id.
- A player sends every lane every block, also while stopped, and a number that hears nothing goes back to its record. Rejected: sending only what moved, as the expression lanes do, because a removed player cannot say it is gone.
- Where a lane bends inside a block, the engine plays the device's block in pieces, so a fast ramp lands on its frame and no device needs code for it.
- How fast a number moves is decided once, in `Lane`, for built-in devices and tools of the project alike.
- Views show the lanes as they play, with the same code the player uses, so a knob and the sound agree.
- A clip moved in the window takes the automation under it, in the same undo step (`arrangement::moved`, a pure function a drag preview also calls). A clip moved by editing its file moves no automation.
- An audio clip plays at its file's speed, so a tempo change moves only its start. Every audio clip edge and jump gets a short ramp, so nothing clicks.
- Selection, snap, zoom, scroll and the clipboard are interface state: not saved, no undo step, invisible to agents. A file is already how an agent shares a clip.

## Audio files and the Sampler

`crates/media`, `extensions/sampler`.

- Audio files a clip names are held in memory, shared, and dropped when nothing names them. Not streamed: the audio thread never waits for a disk, seeks are instant, renders repeat to the byte.
- Our own WAV and AIFF reader, because these formats are a few chunks around plain samples. FLAC is decoded whole on load.
- The Sampler reads the part of SFZ real libraries need and ignores the rest: a third-party file that plays a little plainer beats one that is refused.
- Library samples are memory-mapped and read ahead off the audio thread, so the system pages them from disk. Only files nothing writes in place are mapped, because a mapped file cut short ends the process. Rejected: our own disk streaming with ring buffers per voice, far more code for the same result on an SSD.
- The library is a catalog in code of free instruments at pinned commits, so an id always plays the same. Downloads are shared machine data. Nothing downloads by itself, not for an agent either: an agent's guess must not spend hundreds of megabytes.
- In the window an instrument loads in the background and the old one plays until it is ready. A render, an inspect and the tests load at once, so they play what the records say from the first block.
- Other sample rates go through a stateless resampler, so any block can render on its own from any place. Rejected: resamplers that keep filter state.
- The drawing thread never reads a file; waveform overviews are made in the background and never saved.

## Recording and fit tempo

- MIDI input and the computer keys have no tool and no record. Both write into one lock-free ring the audio thread reads each block, so nothing the interface does can delay sound. Rejected: a tool of the project that plays from the keys, which plays only its own sound.
- A recorded note is saved at the tick the engine sounded it, so playback matches what was heard. The raw performance is also saved as an asset, written before the clip, so it survives whatever happens to the clip. Its times are microseconds, because a tick means nothing without a tempo map and a fit changes the map.
- Audio recording does not pass through the engine. A take is placed by mapping its capture time to the engine frame sounding then, which works with any latency compensation.
- Live input and the sound of other apps are `project.json` connections. Input and output keep their own clocks: the engine drops old frames or plays silence instead of resampling, so the delay stays bounded. The window opens an input only while something needs it, so a project without one never asks for the microphone. Rejected for app sound: a virtual device of our own, which needs a driver install.
- The audio devices are chosen per machine, never in a project. Picking one restarts the app, because the engine runs at the rate of its output device.
- A tempo change ends a take, because the take was heard under the old tempo.
- Fit tempo saves only its inputs in one record. Its outputs go where everything already reads them, the tempo map and the take's clip, through a derive. The beat finder is deterministic, so one take gives the same files on every machine.

## Built-in instruments and effects

One extension per device, one tool with no children. `extensions/filter` is the reference.

- The record is the processor's update, in units an agent can reason about (Hz, dB, seconds, 0 to 1). Every number is one `Parameter` with range and default, which validation, the knobs, the reset, automation and the doc tests all read.
- Every change glides (about 20 ms), including choices, so no edit clicks.
- Shared DSP lives in the SDK (`sound_core`), not in copies: devices kept copying filters, envelopes and helpers. A gain in dB and the pan law are there too, so a pan means the same everywhere.
- Each effect has an exact response function, and tests hold the measured sound to it.
- A time that follows the tempo saves the note, not the seconds, and follows a tempo change while playing.
- A latency is a choice, not a knob: a latency that moves with a drag would make every track jump.
- The Drum pad's default kit is not written into records, so changing the kit changes existing projects. Treat it like a change to a saved format.
- The metronome is not project state: a tool would mean a file in `state/`, and turning the click on would rewrite a musical record.

## Plugin hosting

`extensions/plugin-host`. The core knows no plugins.

- One `plugin` tool for every slot and both formats. Where the track names it decides instrument or effect; what the plugin says only decides which picker offers it, because plugins mislabel themselves.
- A plugin's handle lives on the main thread, so the project lives on one thread and behaviours are not `Send`.
- Nothing a plugin sends out is read, so a plugin cannot make our audio thread allocate. What it changed is asked for on the main thread.
- A pin is a parameter the record holds; the rest stays in the state asset. The record wins over the state. Automation of a pin is never an undo step.
- Scanning runs plugin code, so it runs in child processes, one per bundle with a deadline. A bundle that crashed or hung is remembered and not tried again until the composer asks. The window never waits for a scan.
- Plugin state is not project state: never an undo step, and unchanged bytes are not written again, so a session that changed nothing leaves no diff.
- A slot with no plugin passes its input through: a missing effect lets the sound through, a missing instrument is silent, with no special case in the arrangement.
- Inspect loads no plugin. Printing a project must not run third-party code that can crash.
- A parameter value stays in the format's own units, so what is read from a plugin can be sent back unchanged.
- VST 3 goes through our own safe layer in `src/vst3/`. Bundles are never unloaded, because unloading runs static destructors while views and threads may still exist.
- The host owns every plugin window; the floating option is not supported by real plugins.
- Write "VST 3" in plain text, with Steinberg's attribution line where a VST plugin is offered.

## The window

`crates/ui` and `crates/runtime/src/window`.

- `Session` is the one GPUI entity that owns the `Project`. The core never depends on GPUI.
- Views read state when they render and keep no copy of saved state, so an outside edit shows at once, also mid-drag.
- Views and device offers are registered per tool, so the track panel knows no instrument or plugin.
- The playhead is its own entity, because GPUI redraws a notified view and everything above it.
- `crates/ui` holds the design system and general components; musical interfaces such as a piano roll belong to an extension.
- Extensions, fonts, icons and agent docs are compiled in. "Open project…" starts a new process, because that tears down the device, plugins and their windows correctly for free.
- The app updates itself from the latest GitHub release, with the system's curl and a checked sha256. The swap happens at the next start, because GPUI gives a quit too little time to copy an app. An update that fails to install is not downloaded again.
- No error tracking in the cloud. The window's stdout and stderr go to `log.txt` in the support folder, which the composer can send.

## The agent sidebar

`crates/agent`.

- The sidebar runs Claude Code as a child process in the project folder. It edits files like a terminal agent. Rejected: a sidecar with the TypeScript Agent SDK, which adds a runtime and our own protocol. Rejected for now: ACP, which adds layers and removes none; it is the way to add more agents later.
- The window has a generic left panel slot and names no agent type.
- Each message is one request, so all its writes are one undo step named after the message.
- The thread state is built from provider-neutral events and holds no process, so tests feed it events.
- Threads, settings and whether the panel is open are kept per machine, never in the project: an agent writes in the project folder, and must not change the interface or raise its own access by editing a file.
- The provider keeps the conversation; the app keeps only its session id and a log to show it.
- The app runs the composer's own `claude` when there is one. Otherwise **Set up** downloads a pinned, checked version. Rejected: bundling `claude`, which adds 215 MB to every download and needs an app release to move its version.
- Sign-in is Claude Code's own. The app never reads or keeps a credential.
- New problems after a turn are shown to the composer. Nothing goes back to the agent; its docs tell it to read `problems.txt`.

## Tools of the project

A composer asks the agent for a tool no extension has, and it plays at once, in the installed app, with no build. `crates/typescript`, `crates/signals`.

- **Written in TypeScript, run by Bun as a separate process.** A project with no tool runs no Bun. A file change, a crash or a hang starts a fresh Bun; code that hangs is blamed and left out until the folder changes. The window never waits on Bun. Rejected: reloading code inside Bun, which cannot read a changed import again and has no answer to a hang. Rejected: an engine inside the app (QuickJS), where one endless loop freezes the window and there is no npm.
- **A tool is a JSON tool of the core.** Its records are checked in Rust, so a record loads, and an agent hears what is wrong with it, without Bun.
- **The sound is a graph of signals** built with typed SDK functions, sent as JSON and played by `crates/signals`. Rejected: a text language in between, which nobody wrote by hand and needed a parser. Rejected: that language in a template, which an eval found no better for agents.
- **Fast things never cross to Bun.** Knobs, choices, live controls and triggers change the sound without asking Bun: Bun sends a graph for every combination of choices up front.
- **One signals processor**, made fast by working out once what does not change over a span. Every speed-up keeps every render bit for bit the same; `tooling/perf` measures them. Rejected: compiling to machine code (a JIT), fast but a compiler to own.
- **Cards and pages** are trees of elements Bun draws; the window draws them with `crates/ui`. A `tick` plays like a performer and never changes the record.
- **Keys and MIDI reach the code like a tick.** Rejected: a second tap on the MIDI device thread, which would know nothing of where the notes went.
- **Agents find the docs through the map.** Each tool's doc is written from its fields, so it is never wrong. Rejected: Claude Code skills, which did no better in an eval and need project settings the agent runs without. `tooling/agent-eval` checks agent results.
- **The app ships Bun** at a pinned version, so a composer installs nothing. The agent gets the same Bun first on its `PATH`.
