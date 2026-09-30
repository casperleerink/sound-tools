# Technical architecture

This file holds the technical decisions and the reasons for them: the model, the rules that span crates, and the alternatives we did not take. It does not repeat what the code says; each section points to where the code lives. The product goals are in [CONCEPT.md](CONCEPT.md), tooling and the realtime engine design in [ENGINEERING.md](ENGINEERING.md), the look and the interactions in [DESIGN.md](DESIGN.md). The guides for authors are [crates/core/README.md](crates/core/README.md) (tools and projects) and [crates/ui/README.md](crates/ui/README.md) (views and components). How to run it is in [README.md](README.md).

## Terms

| Term | Meaning |
| --- | --- |
| Sound Tools | The application: a small core plus extensions. |
| Extension | A Rust crate that registers tools, views, device offers and agent docs through the public SDK. Bundled extensions use the same SDK a user extension would. |
| Plugin | A third-party audio plugin (CLAP, VST 3). Never our own extensions. Hosting plugins is the job of one extension. |
| Tool | A composer-facing capability an extension defines. In code, a tool is its saved state type. |
| Instance | One use of a tool in a project, with its own record. |
| Record | The saved JSON state of one instance, one file in `state/`. |
| Behaviour | A function from the state of an instance to what it needs in the engine: processors, updates, connections, ports. |
| Derive | A function from a changed record to more changes in the same edit group. |
| Asset | Bytes under `assets/` the core never looks inside: audio files, raw takes, plugin states. |
| Project runtime | The one process that opens a project folder, owns the live state and the audio engine, and shows the window. |
| Problem | A part of the project that is not live, listed in `problems.txt`. Never a reason to refuse the rest. |

## Shape of the system

- One process, the runtime (`crates/runtime`), opens one project folder. It runs as a window, `--headless`, `--inspect` (print a summary, read-only) or `--render` (offline WAV, read-only). `--plugins` lists the plugins of the machine.
- The agent is an outside coding agent (Claude Code, Codex) run in the project folder. It edits files; the runtime applies them live. There is no separate edit API for agents.
- Crates: `crates/core` (engine, clock, transport, live project folder), `crates/notes` (the note contract), `crates/media` (audio files), `crates/ui` (the UI SDK and the session bridge), `crates/runtime` (the app), `crates/gallery` (component gallery). Each extension is a crate under `extensions/`.
- Every extension is compiled into the one binary. `project.json` enables extensions by name. A new project enables every registered one.
- Rust for the core and extensions, GPUI for the window. The reason: a one-line extension edit reaches a new running window in about two seconds, and an agent wrote working GPUI views from our docs on the first try. Audio and saved data never depend on GPUI.
- macOS is the main platform. Linux builds and passes the tests. Platform code sits behind `cfg(target_os = "macos")`: VST 3 bundle loading, plugin folders, cache folders, the terminal, and plugin windows (macOS only).
- Sound Tools is a standalone app. Running it as a plugin inside another DAW is out of scope.

## The core knows no music

This is the main rule of the codebase.

- The core has no track, clip, note, pitch, velocity, effect, plugin or audio file type, and depends on no bundled crate. `workspace-rules` checks this.
- Extensions never depend on each other. What two extensions both need lives in a contract crate: `sound-notes` (saved `Note` and `Clip`, the realtime `NoteEvent` with the bend and mod wheels and key pressure, the raw MIDI take, the port names of instruments and effects, and how the bundled instruments play notes: `Voices` and `Wheels`) and `sound-media` (reading, resampling and pitching audio files).
- Tools find each other by port names, not by type. An instrument has an event input `notes` and an audio output `audio`. An effect has an audio input and output both named `audio`. So any tool with those ports fits a track slot, and the arrangement depends on no instrument, effect or plugin.
- The core does own the musical clock (tempo map, time signatures, ticks). Nearly every tool and agent request talks in bars and beats, so one clock in the core beats one per extension.
- `extensions/tone` is a small non-musical tool. It is not in the default project; it proves the core rules hold for a tool shaped differently from the arrangement.

## Tools, instances and behaviours

Built in `crates/core/src/project`.

- A tool is its saved state type (`State`). There is no separate tool handle to mix up. A tool belongs to one extension, and its records load only when that extension is enabled.
- A behaviour is declarative. It runs for every valid state from every source (file, window, undo, load) and declares everything each time. The core keeps what was declared before and sends only the difference. So a behaviour cannot leak processors, deleting an instance removes what it made, and a processor declared again keeps its phase and voices.
- A parent's behaviour reads the typed state of its owned children and runs again when anything below it changes, children first.
- One edit group runs all its behaviours in one engine batch. If a behaviour or the graph refuses, nothing of the group applies. Two exceptions keep one bad file from blocking a whole project: on open, an instance whose behaviour fails is left out and reported; a `project.json` connection that closes a cycle is kept, unused and reported.
- A behaviour can report a problem (`BehaviourContext::problem`) without failing the edit. This is how "a missing plugin or file is reported, that slot is silent, and the rest plays" holds in one place. Refusing is for invalid state; a problem is for state that is valid but cannot be live now.
- Ownership is the folder tree. References (connections, a clip's take) are saved ids that resolve to an optional instance and keep nothing alive. A reference may arrive before its target; data of a missing extension stays intact.
- `Project::rebind` runs one behaviour again with the record it has, with no write and no undo step. It is for a service outside the project that can now do more: a plugin scan that found the plugin, a drum sound rendered in the background. `rebinds_on_assets(folder)` does the same when a file arrives under `assets/<folder>/`, only for instances that have a problem.
- A tool may say where its instances live (`Place`): anywhere, at the top, only inside one named tool, or at one fixed id. A record in the wrong place is a problem that says where it belongs, so an agent's mistake is a message and not silence.
- A derive (`ToolRegistration::derive`) turns one changed record into more changes in the same group, so the result is one engine batch and one undo step. It runs when its record or the time signatures change, never on load, undo, redo or cancel, because the files already hold its output. It cannot loop and cannot fail: what it cannot compute is a problem. It is told what the record was, so it writes only what moved. Rejected: running derives on load (cost on every `--inspect`, and it could silently disagree with the files).
- A tool may register a summary (for `--inspect`) and an end (for the transport length). This is how the core prints "what plays where" without knowing tracks.

## Project storage

A project is a folder of JSON records and assets. Together with its extensions it is the whole piece. Built in `crates/core/src/project/storage.rs` and `file.rs`.

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
```

- The folder tree is the ownership tree, so one parent per child and no cycles come for free. The id of an instance is its path under `state/`. Display names live in the record, so a rename never moves a file.
- The tool decides the form for good (`State::OWNS_CHILDREN`): always a folder with `instance.json`, or always one file. The runtime never moves a record between forms, because a path that changes under an agent leads to ignored writes and deleted records coming back.
- Adding a part is one new file, moving a clip to another track is moving a file, adding a track is one new folder. The format is chosen for agents and for git.
- The runtime writes JSON with stable key order and one line per item when a list is long, so a clip has one note per line and a small edit is a small diff. Any valid JSON loads.
- There is no instance index. The runtime reads `state/`.
- No schema versions and no migration framework. Keeping code and saved data compatible is the composer's and their agent's job. Old records must load unchanged: a new field is optional with a default that sounds as before, and the runtime does not rewrite a file that did not change.
- A change of `extensions` in `project.json` is refused while the project runs and applies on reopen. An invalid `project.json` stops the project from opening. Anything else that does not load is a problem, and the rest opens.
- What belongs to one machine stays out of the folder: the plugin scan cache and plugin window positions live in the user cache folder (`~/Library/Caches/sound-tools/`, `~/.cache/sound-tools/`). A project in git must not change because it was opened on another laptop. For the same reason the generated agent docs contain nothing of the machine.
- Assets are bytes the core never reads. `AssetName` is checked so a name from a record can never point outside the project. Assets are created with `create_new` and numbered names, never written over by accident, never deleted by the runtime, and never part of undo. So no recorded performance or imported file can be lost by an edit.
- Each file is written to a temporary file and renamed into place. No `fsync`: it made undo of a large delete block for most of a second. Power loss is left to git. Parents are written before children, then deletions, then `project.json`. A crash leaves at most one stale record, which loading reports.
- One runtime per project holds `.sound-tools.lock`. `--inspect` and `--render` open read-only without the lock and write nothing, so they work next to a running window.

## The live folder and editing

Built in `crates/core/src/project` (`editing.rs`, `outside.rs`, `watcher.rs`).

- There is one state application. Window edits, file changes, loading, undo, redo and cancel all call it with a group of changes, and it applies the group whole or not at all. Loading is applying from empty.
- The watcher only says which paths changed. The runtime reads them again and applies the difference. Event kinds are not used, so edits, new files, deletes and moves take one road. Changes that come less than 100 ms apart are one group; the runtime groups raw events itself, because a debouncer can split a burst.
- The runtime recognises its own writes by a fingerprint of the bytes, and a file that decodes to the current state is no change.
- Last write wins everywhere: file edits, open drags, undo, redo, cancel. There is no field merge and no conflict rejection. Do not reopen this as a sync problem.
- An edit is `begin`, `publish` (applies live, writes nothing), then `finish` (writes and adds one undo step) or `cancel`. Each record keeps a committed state, so no undo step ever holds the middle of a gesture, even when a file edit lands during a drag.
- Outside changes within 15 s of each other, with nothing else in between, are one undo step. Agents write the files of one request seconds apart; without this, one request became several undo steps. This is a heuristic until an agent can say where its request ends.
- Undo history lives only in the session. The folder is the truth; versions come from git.
- `project.json` is written whole, so an unseen outside change is read in first, and a `project.json` that holds an outside change that did not load is not written over.
- `problems.txt` exists while a runtime holds the project, says "No problems. Every file is live." when all is well, and is removed on a clean close. So an agent can tell "all is well" from "nobody is watching".

## Agent context

- The runtime writes `AGENTS.md` (the map: layout, ids, ticks and bar math, how to write a file, how to check `problems.txt`, and a table of docs with one line on when to open each), `CLAUDE.md` (imports the map), and one file per doc in `agent-docs/`. Built in `crates/core/src/project/generated.rs`.
- A map plus docs, not one big doc, because one doc stopped scaling with the extensions. Agents read the map and then only the docs their task needs.
- Extensions register their docs (`registry.agent_doc`); the core brings the doc for `project.json`. The sources are product content: `extensions/*/agent-doc.md` and `crates/core/src/project/*.md`. The runtime owns every `.md` in `agent-docs/` and removes one no enabled extension registers, so a doc never outlives its tools. Files are written only when the text changes.
- A runtime test loads every JSON example in every doc, so the docs cannot drift from the formats.
- The project menu opens a terminal in the project folder (the system Terminal on macOS, `$TERMINAL` or `x-terminal-emulator` on Linux), so the composer can start an agent there.

## Audio engine, transport and time

The threads, messages and schedule compile are in [ENGINEERING.md](ENGINEERING.md) section 3. Built in `crates/core/src` (`engine.rs`, `processor.rs`, `clock.rs`, `transport.rs`).

- One audio engine. `process` never allocates, locks or makes a system call; values leaving the audio thread are dropped on the control thread. Every crate with a processor runs its tests under the realtime sanitizer in CI.
- Every audio port is stereo. No mono ports means no channel negotiation, a stereo cable cannot be half connected, and a plugin's stereo output fits one port.
- Engine time is an integer frame count. Musical time is an integer tick count, 960 per quarter note. Saved positions are ticks, never floats or seconds. Tick to frame rounds in one place, and each clock segment keeps the fraction of a frame across tempo changes, so a map with a change on every beat stays exact.
- The tempo map is a list of step changes and the time signatures. A tempo change during playback keeps the position in ticks.
- Time signatures are runs of whole bars (`{"signature": "7/8", "bars": 2}`), and the last run goes on. So a change can only fall on a bar line and no position has to be checked against one. A change every bar, as in the Danse sacrale, is one run per bar. Rejected: changes at ticks, which could fall inside a bar.
- Changing the time signatures moves bar lines, never notes: clips keep their ticks. Keeping bar positions would need a derive that moves every clip, and it would go wrong for clips an agent placed off the bar lines.
- Bar math (`bar_at`, `bar_beat_of`, the grid, the ruler, the click) lives in `TimeSignatures`, so no extension assumes a fixed bar length. `AGENTS.md` explains the math and points at `project.json` instead of copying the time signatures, and `--inspect` prints where each one starts.
- Each block, a processor gets the range of ticks it covers and the frame of any tick in it, so no extension rounds time itself. The core holds no scheduled events: timeline processors make their events block by block, so a seek has nothing to invalidate.
- Transport changes are two one-block flags: the position jumped (seek, stop) and playback stopped (pause, stop). Tools release what they hold on either. Seeking does not replay skipped events.
- The engine runs every processor every block, playing or not, so live keys and effect tails keep sounding while stopped.
- A routing edit sends a new schedule; processors that survive keep their state. Removing a processor is a hard switch, with no crossfade.
- The graph has no cycles. A connection that would close one is reported and not used.
- Latency compensation leads instead of delaying. Each processor sees the transport ahead by the latency between it and the device, so a sequencer before a 700-frame plugin sends notes 700 frames early. No delay lines and no maximum. Delaying after a track would also delay a keyboard played into it; leading keeps live playing at the cost of only that track's own chain. After play or seek the playhead waits for the longest latency. A render leaves that wait out, so tick 0 is frame 0 of the file.
- Levels leave the audio thread through `Peaks`: atomics, no messages, no missed peak. Views read them once per poll and draw only when the reading changes.
- An offline render tells processors it is offline (`PrepareConfig::offline`), so a plugin that streams samples from disk waits for them instead of playing silence.
- Every render is deterministic: the same project gives the same bytes. Nothing random, LFOs start at phase 0, and the click is attached only by the window, never by `--render`, `--inspect` or `--headless`.

## The arrangement

`extensions/arrangement`.

- It owns tracks, note clips, audio clips, the track mixer (volume, pan, mute, solo) and the master. Mixer values are fields of the track record, not a tool, because they are the track.
- A track's chain is its instrument (the child named `instrument`), then the effects its `effects` list names, in that order. One list decides the order, so a reorder is one record and one undo step. Rejected: an order number in each effect record (two files per move, and ties).
- Bypass is saved on the track's effect slot, not in the effect's record, so plugins and built-in effects share it. A bypassed effect leaves the chain and takes its latency with it.
- Solo gives the other tracks the gains of a muted track in the same mixer. That is why the mixers moved from the tracks to the arrangement: only the owner of every track can see them all.
- The master's limiter is code of the arrangement, fixed at the end, not a tool. Old projects get it without a file changing. Under its ceiling it is bit-transparent. Its DSP is `PeakLimiter` in the core, a per-frame helper like `Smoothed`, which the Limiter effect uses too, so both sound the same. The core itself limits nothing. Things connected to the device by hand and the click go around the master.
- Clip rules: note starts count from the clip start and every note starts inside its clip (else the record does not load), so a note written with a project position is an error and not silence. Overlapping note clips all play. No note is ever stuck: an edit, move, delete or tempo change ends what it started, and `AllOff` ends everything, pedal included.
- An audio clip has no length of its own; it plays at its file's speed, so a tempo change moves its start only. Overlapping audio clips: the highest `layer` is heard. The layer is in the record because file times are not stable across copies.
- Every audio clip edge, hand-over and jump gets a short ramp or crossfade, so nothing clicks.
- Selection, snap, zoom, scroll and the clipboard are interface state: not saved, no undo step, invisible to agents. The clipboard is in the app only, because another app has no use for a clip and a file is already how an agent shares one.

## Audio files

`crates/media`.

- Audio files a clip names are held in memory once, shared by every clip, and dropped when nothing names them. Not streamed: nothing on the audio thread ever waits for a disk, seeks are instant and renders repeat to the byte. Memory is bounded by the size of the files. Streaming could come behind the same read call if a project ever needs it.
- Our own WAV and AIFF parser. These formats are a few chunks around plain samples; a decoder library would only add a copy.
- A file at another rate plays through a stateless windowed-sinc resampler, so any block can be rendered on its own from any place. Rejected: block resamplers that keep filter state (a clip is heard from wherever the playhead is).
- The Sampler pitches with a separate `Varispeed`, because a key's step is not a ratio of two rates.
- The thread that draws never reads a file. It asks a cache that answers from memory; waveform overviews are made in the background and never saved.

## Recording and fit tempo

- MIDI input (`extensions/midi`) has no tool and no record. A device thread writes into a lock-free ring; the audio thread plays what is there at the start of the next block. Nothing the interface does can delay sound.
- A recorded note is saved at the tick the engine sounded it, so playback matches what was heard. The raw performance is also saved as an asset (`assets/takes/`), named on its own and referenced by the clip, written before the clip, so it survives whatever happens to the clip.
- Raw take times are microseconds, never ticks, because a tick means nothing without a tempo map and a fit changes the tempo map.
- Audio recording has no monitoring and does not pass through the engine. The device callback writes into a lock-free ring with the capture time; a recorder writes files in the background. A take is placed by mapping the capture time to the engine frame that was sounding then, and from there to the project frame. This needs no engine change and works with any latency compensation.
- A tempo change ends a take, as a seek does, because the take was heard under the old tempo.
- Fit tempo (`extensions/fit-tempo`) saves only its inputs in one record: which take, the first downbeat (in take microseconds, so an agent can copy it from the take), half/normal/double, and steadiness. The outputs go where everything already reads them: the tempo map and the take's clip, through a derive. One fit per project, fixed by `Place`. Steadiness never touches the clip, so 0 gives the fitted map back byte for byte.
- The beat finder is deterministic (a small Ellis 2007 dynamic-programming tracker on note times) and simulates the grid at a fixed 48 kHz, so one take gives the same files on every machine.

## Built-in instruments and effects

The synth (`extensions/instrument`), Sampler, Drum pad, Filter, Compressor, Limiter, EQ, Delay, Reverb, Saturator, Utility and Modulation all follow one pattern; `extensions/filter` is the reference.

- One extension per device, one tool with no children, found by port names.
- The record is the processor's update, in units an agent can reason about (Hz, dB, seconds, 0 to 1). Every number is one `Parameter` constant with range and default, which validation, the knobs, the reset and the doc tests all read.
- Every change glides (about 20 ms, `Smoothed`), including choices, so no edit clicks.
- What a processor uses per frame is in the SDK, next to `Smoothed`: `Envelope`, `Lfo`, `DelayLine`, and in `dsp.rs` the fading `Taps` of a delay line, `OnePole` and `held`. The filter and the Modulation share one LFO, the reverb, the Modulation and the Delay one delay line, the reverb and the Delay the taps, the cuts and the hold.
- An instrument's notes go through `sound_notes::Voices`: which voice plays which key, voice stealing, the sustain pedal, `AllOff`, mono with legato, and glide. The instrument keeps the sound of a voice (`Voice`). The synth takes over a voice in place; the Sampler cuts it and fades it out beside the new note, because a sample cannot jump to its start without a click.
- The Modulation glides rate, depth and spread over 100 ms: they move where its delay is read, and a read that moves fast bends the pitch.
- Each effect has an exact response function, and tests hold the measured sound to it.
- A gain in dB and the pan law are in `sound-core` (`amplitude`, `pan_gains`), so a pan means the same on a track, a drum pad and the Utility.
- The card is drawn from shared UI components; every control goes through `ControlEdit`, one gesture per drag.
- A time that follows the tempo, such as a synced Delay, reads the tempo of the transport where each block starts, so it follows a tempo change while playing and while stopped. The record saves the note, not the seconds.
- A new delay time never moves a read position: the read fades over 20 ms from the old tap to the new one, so nothing clicks and no pitch slides.
- A lookahead or delay is reported as latency; latency is a choice, not a knob, because a latency that moves with a drag would make every track jump.
- The Drum pad renders its sounds whole on a background thread and plays buffers on the audio thread. No sound is made inside an edit. Its default kit is not written into records, so a change to the kit changes how existing projects sound. Treat it like a change to a saved format.
- The metronome is not project state: no tool, no record, no undo step. A tool would mean a file in `state/`, and turning the click on would rewrite a musical record.

## Plugin hosting

`extensions/plugin-host`. The core knows no plugins.

- One `plugin` tool for every slot and both formats (CLAP, VST 3): the format, the plugin id and a `state_asset` name. The same record is an instrument or an effect depending on where the track names it. What the plugin says it is only decides which picker offers it, because plugins mislabel themselves.
- A plugin's handle lives on the main thread; only its audio processor goes to the audio thread, and it is stopped there before it leaves (`Processor::leaving`). The project therefore lives on one thread and behaviours are not `Send`.
- Nothing a plugin sends out is read (a void event list), so a plugin cannot make our audio thread allocate.
- Scanning runs plugin code, so it runs in child processes (the runtime itself), one per bundle, with a deadline. The window scans in the background and never waits; a record whose plugin is not found yet is reported and rebinds when the scan finds it. The cache belongs to the machine.
- Plugin state is saved when the plugin marks it dirty and when it goes or the project closes. It is not project state: never an undo step, and bytes already there are not written again, so a session that changed nothing leaves no diff.
- A slot with no plugin passes its input through. So a missing effect lets the sound through and a missing instrument is silent, with no special case in the arrangement.
- `--inspect` loads no plugin, only checks it exists and its state reads. Printing a project must not run third-party code that can crash.
- VST 3 goes through our own safe layer in `extensions/plugin-host/src/vst3/`; nothing outside it touches a VST 3 interface. Bundles are loaded once and never unloaded, because unloading runs static destructors while views and threads may still exist.
- The host owns every plugin window (both formats embed a view; the floating option is not supported by real plugins). Windows float above the main window only. The plugin's view is freed from an `on_window_closed` observer, the one path that runs however the window goes; getting that order wrong is a use-after-free in native code.
- Window positions and open state are machine data, stored next to the scan cache by project path.
- Write "VST 3" in plain text, with Steinberg's attribution line where a VST plugin is offered.

## The window

`crates/ui` and `crates/runtime/src/window`.

- `Session` is the bridge: one GPUI entity that owns the `Project` on the main thread, polls the engine and watcher, and emits project events. `crates/ui` depends on the core; the core never depends on GPUI.
- Views read state when they render and keep no copy of saved state, so an outside edit shows at once, also mid-drag. Controls are controlled components.
- Views and device offers are registered per tool in two GPUI globals, `Views` and `Devices`. The track panel asks them what to show and what can go in a slot, so the arrangement knows no instrument or plugin.
- The playhead is its own entity next to the cached timeline, because GPUI redraws a notified view and everything above it. While playing, the timeline code does not run.
- The arrangement paints on one canvas, only what is visible, with its coordinate math in pure tested functions.
- Undo and redo do nothing while a gesture is open; one gesture at a time.
- The UI SDK (`crates/ui`) holds the shared design system: colours, type, and general components such as knobs, cards, menus and displays. A slider belongs there; a piano roll belongs to an extension. Extensions may still use GPUI directly for custom musical interfaces. The rules are in DESIGN.md.
- The macOS app bundle is the runtime binary alone. Extensions, fonts, icons and agent docs are compiled in. With no folder it opens the last project. "Open project…" quits and starts a new process, because a new process tears down the device, plugins and their windows correctly for free.

## Direction: extensions made by the agent

The goal is that a composer asks an integrated agent to create or adapt extensions for their piece: a new tool, instrument, effect or view. The extension system exists for this. Bundled extensions use only the public SDK, so an agent-made extension has the same power. The live project folder exists for this too: content edits apply at once, with no build.

- Composing edits project state and never needs a build. Changing an extension changes code and goes through build and reload, where a few seconds and a restart are fine.
- A build happens while the composer keeps using the running runtime. On success, playback stops, the runtime restarts and reopens the project from the folder. A failed build reports errors and keeps the old executable.
- Each project keeps its own copy of the extensions it uses, so changing one for a piece never changes another project.
- The runtime and the SDK stay independent of the agent implementation.
