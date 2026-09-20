# Technical architecture

This document records architecture decisions and proposals. The product goals are in [CONCEPT.md](CONCEPT.md). Revised September 14, 2026: the v0 is a small DAW with an agent sidebar, built from bundled extensions on a small core. Revised September 19, 2026: the first milestone is cut down and the saved time format, record size and watcher scope are decided. The small GPUI build-loop experiment below has been validated on macOS. An isolated core lifecycle prototype now implements typed state, persistence, editing, offline processing and GPUI views for Tone and an agent-authored Tremolo. The realtime engine from ENGINEERING.md section 3 is built in `crates/core`, with the musical clock, the transport and the live project folder: tool registration, instances, editing with undo, storage and the file watcher. Tone runs on it as the first tool. The application window shows the arrangement live with the transport and the project menu, and a composer adds, moves, resizes and deletes clips and notes in it with the mouse and the keys. The first milestone was verified on the real application on September 19, 2026, with Claude Code and Codex as outside agents. The agent sidebar and the outer application are not built yet.

## Terms

| Term | Meaning |
| --- | --- |
| Sound Tools | The application, consisting of the core and installed extensions. |
| Extension | A Rust package that provides tools and supporting functionality, such as shared types and UI components. Compiled into the project runtime with the SDK; its source travels with the project. Projects choose which extensions they use. |
| Bundled extension | An extension that ships with Sound Tools and is part of the default project template. It uses the same SDK as user extensions. |
| Plugin | A third-party audio plugin (VST, AU, CLAP): a separate binary with a fixed interface loaded by a host. Sound Tools does not use this word for its own extensions. Plugin hosting belongs to an extension. |
| Tool | A composer-facing capability defined by an extension, ranging from a small building block to a complete composition workflow. |
| Project template | A starting configuration for new projects, including a selection of extensions and optionally initial content and workspace layout. |
| Project | A saved musical workspace containing its extension selection, content, settings and assets. |
| Outer application | The application that manages agent sessions, project files, extension development, builds and the project runtime process. |
| Project runtime | A separate process running the Rust core and a project's enabled extensions, with or without musical interfaces. |
| Tool instance | One use of a tool in a project, with its own saved content, settings and connections. Also called an instance. |
| Project state | Everything needed to restore the work between sessions. |
| Runtime state | Temporary information during execution, such as active voices or delay buffers. This can reset when reopening. |
| Parameter | A named, typed value that controls an instance. |
| Event | Something that happens at a particular time, optionally carrying typed data. |
| Signal | A continuous stream of sampled values between processors, used for audio or modulation. |
| Connection | A link between compatible outputs and inputs. |

## Language and UI

Rust is the preferred language for the core and extensions. Extensions are trusted user code.

Target macOS, Windows and Linux desktop. macOS is the primary development and initial validation platform. Keep platform-specific integration separate from the shared core and SDK; verify the other desktop platforms explicitly before claiming support.

Sound Tools is a standalone application. Running Sound Tools itself as a plugin inside another DAW is outside the target architecture.

GPUI is the provisional UI choice. Validate whether an external coding agent can create and modify extension interfaces using the SDK documentation and examples.

The UI SDK includes a small design system and built-in UI components for extension authors and agents. Direct GPUI access remains available for custom musical interfaces. Audio execution and saved musical data remain independent of GPUI.

Use one window with an agent sidebar and the project's musical application in the main area. The application supplies this basic layout; extensions define the musical workspace and can provide transport controls. A fixed top bar is not required.

### UI design system

Agents should start from shared UI guidance, components and examples. The lifecycle probe demonstrated extension authoring, but its ad hoc styling and controls are not the intended design baseline.

Start with shared colours, typography, spacing and interaction states, plus buttons, labelled controls, numeric inputs and parameter sliders. Shared controls should provide consistent keyboard behaviour, visible focus and accessible labels. Extensions should use them for ordinary interactions and use direct GPUI when a custom musical interface needs it.

Keep the components general. A slider belongs in the UI SDK; a prescribed track editor or piano roll belongs to an extension. Exact styling and component APIs remain to be tested through real tools.

Verify the first small set by using it in Tone, then asking a Codex subagent to adapt Tremolo using the same components and guidance. Check visual consistency, keyboard operation, focus, accessible labels and whether extension UI code gets smaller. Grow the set from demonstrated needs. The design system is an agreed direction, not implemented functionality yet.

## Outer application and project runtime

Parked September 20, 2026. Nothing in this section is built. The product is one process, the project runtime, and the agent is external. The section stays as the direction for when extension builds and an in-app agent come back into scope.

The outer application and project runtime run as separate processes. The outer application manages the project folder, agent conversation, extension source code and builds. It starts and stops the runtime and remains available while the runtime rebuilds or restarts.

Support one open project at a time for now, with one active project runtime and one audio engine. Simultaneous project sessions are not required.

The project runtime runs the Rust core and enabled extensions. It owns live musical state, the audio engine and the tools' custom interfaces. It can also run without graphical interfaces for project inspection, editing and audio rendering by agents.

A small protocol connects the two processes: start, stop and reload the runtime, transport operations, and reporting of state and errors. Project edits do not go through this protocol. Agents edit the project files directly and the runtime applies the changes live, so external agents can edit a project without opening the graphical application.

The runtime is the single owner of live musical state and keeps the project folder current. On reload the replacement runtime restores the project from the folder. Agent conversation state survives the reload in the outer application.

The provisional rendering approach lets the runtime draw the entire window, including the agent sidebar. The outer application manages the agent session in the background and supplies conversation data to that sidebar. This keeps the visible interface in one process while preserving agent sessions across runtime restarts. The window may briefly close and reopen on reload; validate the presentation and reload mechanics with a prototype. The communication protocol remains open.

Users should be able to sign in to different AI providers and use their supported subscriptions through the integrated agent. Interchangeable coding agents are not a requirement. Building on Pi is an acceptable direction, subject to verifying its integration and the provider subscription flows we need. The project runtime and extension SDK remain independent of the agent implementation. Specific providers and authentication support remain to be decided and verified.

## Builds and reloads

Two kinds of work have different speed requirements. Composing within a project edits project state: instances, connections, parameters and extension-defined content. These edits apply immediately in the running runtime, whether they come from an interface or from the agent, and never require compilation. Building or changing an extension changes code and uses the build and reload workflow; build time and a restart are acceptable there. Bundled extensions should cover common musical needs with compiled processors and helpers, so most composer requests are project edits rather than code changes.

Changes to extension code or enabled extensions may stop playback and use the project reload workflow. Routing edits within a project do not: the engine receives a new schedule and surviving processors keep their state, as designed in [ENGINEERING.md](ENGINEERING.md) section 3. This replaces the SDK sketch's earlier fallback of stopping playback on routing edits. Seamless replacement of running code or audio graphs is not required. Recompile when code or build dependencies change; reloading existing compiled functionality does not inherently require compilation. Parameter changes during playback must work without compilation or project reload.

The proposed build structure separates the engine, SDK and application UI, with one crate per extension initially. All projects share one Cargo target directory per machine, so the dependency tree compiles once on first launch or after a toolchain change. Opening a new project compiles only its own extension crates and the runtime binary.

Compile enabled extensions into the project runtime executable. The composer can keep using the current runtime while the agent edits source and builds. After a successful build, the outer application automatically stops playback, waits for the runtime to finish writing the project folder, then restarts the runtime and reopens the project with playback stopped. A failed build reports errors and retains the previous working executable. Changes using already compiled functionality need no compilation. Validate build and restart behaviour with a prototype.

### Build-loop experiment, September 9, 2026

A throwaway workspace with one GPUI runtime crate and one statically compiled extension measured the loop on a development Mac. A one-line extension edit reached the replacement runtime's first frame in a median 2.2 seconds, with 1.3 seconds of that in the incremental build and link. A failed build kept the old process alive and the executable unchanged. An agent wrote a two-instance custom GPUI view that compiled on its first attempt from public docs.

A follow-up put a small oscillator and filter in the edited extension crate with `opt-level = 3`. Seven DSP gain edits reached the replacement frame in a median 2.23 seconds, including 1.35 seconds for build and link. Each replacement rendered 48,000 samples before opening its window, verified finite bounded output, and reported the changed gain. Only the extension and runtime rebuilt; the old runtime survived every build. This measures a small offline DSP workload, not an audio callback or full release build.

This supports keeping Rust and GPUI. Not yet measured: larger DSP workloads and extension sets, live audio readiness, and accessible custom controls. A build into an empty target directory took 56 seconds, which is the one-time cost of the shared target directory. Method, setup failures and raw evidence are in [the experiment folder](experiments/gpui-build-loop/README.md).

### Core lifecycle prototype, September 9, 2026

[The isolated prototype](experiments/core-lifecycle/README.md) implements a small core alongside Tone, then tests its SDK by having a Codex subagent author Tremolo with its own custom view. The author compiled it and passed its two integration tests on the first attempt, without changing the core or UI bridge. A documentation gap in the public project lifecycle signatures was found and filled.

Five integration tests cover editing, offline processing, persistence, last-write-wins, undo/redo and restoration. A successful optimized DSP edit took 2.14 seconds through the replacement frame and preserved saved records and routing; an intentional compile failure retained the old runtime and executable. No built-in agent, provider login or agent sidebar was needed for this authoring test.

Native automated interaction exposed a repaint issue: records and notifications update, but screenshots remain stale until resizing the window. Automatic visible refresh and accessible controls remain unverified. The engine in this slice only renders offline on one thread; it does not validate device output or realtime processing. These findings guide the next core work rather than fixing the SDK in advance.

## Tools and extension composition

Tools are the units composers create, agents discover and projects save. An extension registers one or more tool definitions. Registration makes types available; creating an instance gives a tool its project-specific state.

A tool can associate saved state, behaviour, views, actions, parameters and exposed ports. These capabilities are optional. Processors, scheduling code and UI components implement the tool; they do not each require a separate extension. A tool can contain other tools and offer a combined interface.

Exposed inputs and outputs define how a tool interacts with other tools. They need not reveal its internal processing structure. The core executes the underlying processors and scheduled events independently of the visible interface.

For example, a rhythmic loop tool defines how loops behave, how they are edited and which trigger ports are available. Its instances save the composer's patterns, playback settings and connections to other tool instances.

Ordinary Rust dependencies support code reuse. Connections between tool instances are project state. Two tools can communicate through a shared port contract without depending on each other's implementation. Shared contract crates should follow concrete interoperability needs.

Fixed internal processing structure can be reconstructed from extension code and saved settings. Composer-editable structure belongs in project state. Child tool instances use the core's persistence facilities; avoid saving duplicate representations of the same content.

The project manages tool instances. A composite tool can explicitly own child tool instances; deleting the parent also deletes those children. Connections and references do not establish ownership, so deleting a connected tool does not delete its peers. Views reference musical content without owning a second copy, and closing a view does not delete that content.

### Registration and lifecycle, decided September 19, 2026

Built in `crates/core/src/project`. The guide for extension authors is [crates/core/README.md](crates/core/README.md).

- A tool is its saved state type. The type implements `State`, which names the tool and validates. Every typed call names the type, so there is no separate tool handle to pass around or to mix up. An extension registers its tools in a `Registry` before the project opens. A tool belongs to one extension, and a project loads its records only when `project.json` enables that extension.
- A behaviour is optional. It is one function from the state of an instance to what the instance needs in the engine: processors, updates, its own connections and named ports. It runs for every valid state from every source, and it declares everything each time. The core keeps what was declared before and sends only the difference. So a behaviour cannot leave processors behind, deleting an instance removes what it made, and a processor that is declared again keeps its phase and voices. A tool with no behaviour is plain data for its owner.
- A behaviour reads the typed state of its owned children and the ports they expose. It runs again whenever its own record or anything below it changes, children before parents. So a parent can build one snapshot from many child records and route to a child, and it goes to the device by itself with no `project.json` edit.
- One edit group runs all its behaviours inside one engine edit: one batch and at most one compile. If a behaviour or the graph refuses, nothing of the group applies. Two exceptions keep one bad file from blocking everything. While a project opens, an instance whose behaviour fails is left out with what it owns, and reported. And a `project.json` connection that closes a cycle never fails an edit: it stays saved, unused and reported, like a connection to a missing instance.
- Two instances may declare the same connection, for example a parent and its child both send the child to the device. The graph holds it once, and it goes when the last one stops declaring it.
- References are saved instance ids. They resolve to an optional typed instance, own nothing and keep nothing alive. `project.json` connections are references too: an end that does not exist leaves the connection saved, unused and reported, so a connection may arrive before its instance and the data of a missing extension stays intact. Deleting an instance removes the connections that name it, in the same undo step.
- Not built: a behaviour that reacts to changes of an instance it only references. Parents and children cover the milestone.

Audio plugin hosting belongs entirely to extensions. A bundled plugin host extension provides it for the v0 DAW; the core has no plugin interface. The hosting design is not specified here.

## v0: DAW workspace from bundled extensions

The v0 workspace is a small DAW. Its parts are bundled extensions that ship with the product and are part of the default project template. They use the public SDK only; nothing in them is a core feature.

| Bundled extension | Provides |
| --- | --- |
| Arrangement | Tracks, clips, notes and automation on the core timeline, with an arrangement view and a clip or note editor. |
| Mixer | Sends, buses and the mixer view. The gain, pan and mute of a track are in the arrangement, in the track record, see "Stereo signal path and the track mixer". |
| Instrument | One subtractive synth with a few parameters. |
| Sampler | Plays imported samples from project assets. |
| Effects | Two or three, such as delay, filter and reverb. |
| MIDI input | Maps MIDI devices to instrument tracks. |
| Plugin host | Loads third-party audio plugins (VST3, AU, CLAP) as instruments and effects on tracks. |

Build the core and these extensions together. Each extension should be small and finished before starting the next. Order: arrangement and instrument first, since they prove the note contract, the musical clock and live agent edits. Plugin host last, since it depends on the note and audio contracts being stable.

The arrangement saves a folder per track and a file per clip. A track record holds its name, colour and order. A clip record holds its own start, length and notes, one note per line, so adding a part is one new file and moving a clip to another track is moving a file. A note line holds its start and length in ticks (the length is at least 1), a MIDI note number and a velocity from 1 to 127, for example `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`. A track owns its instrument as a child instance and goes to the main output by default, so adding a track is one new folder and no `project.json` edit. The headless inspect command prints a project summary, so agents do not need to open every clip to answer what plays in a bar range.

The arrangement extension's saved format becomes the de facto note and clip contract other extensions read. It lives in a bundled contract crate, not in the core. The core stays independent of notes, tracks and clips.

Decided September 19, 2026, the note contract crate: `crates/notes`, package `sound-notes`. The arrangement and every instrument depend on it and not on each other. It holds:

- The saved `Note`. Pitch (0 to 127), velocity (1 to 127) and length (1 tick or more) are types that cannot hold a wrong value, and they save as plain numbers.
- The saved `Clip`: its start and length in ticks and its notes. It moved here from the arrangement with the arrangement build, because its saved form is what other extensions read. The tool name stays `arrangement.clip`.
- The realtime `NoteEvent`: `On` with pitch and velocity, `Off` with pitch, and `AllOff`. A sender sends `AllOff` when the transport stops or jumps, and on one frame it sends offs before ons. A sender whose notes can change while they sound, such as a track, keeps a fixed list of the notes it started, so each gets its off.
- Pitch to frequency: twelve equal steps per octave, A4 at 440 Hz.
- The port names of an instrument: an event input `notes` and an audio output `audio`. An owner finds its instrument by these names, so any tool with these ports fits. The audio is stereo, like every audio port, since step 1 of the second milestone.

The instrument extension is `extensions/instrument` with the tool `instrument.synth`. Its saved state uses units an agent can reason about: Hz, seconds and 0 to 1. `extensions/instrument/README.md` is the guide for editing a synth record.

Decided September 19, 2026, with the arrangement build in `extensions/arrangement`. Its README has the rules of the sequencer, and its `agent-doc.md` is the single source for the record formats.

- Three tools: `arrangement` (the root, owns tracks, no settings yet), `arrangement.track` and `arrangement.clip`. A track finds its instrument as the child named `instrument`, by the port names of the note contract, so the arrangement depends on no instrument. A track without one loads and is silent.
- Clip rules, the simplest that hold: note starts count from the clip start. Every note starts inside its clip, else the record does not load, so a note written with a project position is a reported error and not silence. A note that is longer than the rest of its clip ends with the clip. Clips on one track may overlap and all their notes play. Two sounding notes of one pitch sound until the last of them ends, with or without an edit in between. A seek never starts a note in its middle.
- Track colour is a name from the DESIGN.md accents, an enum, `blue` when left out. Track order is a whole number, 0 when left out. Tracks show by order, then by id.
- No stuck notes: a note that started gets its off when its clip is edited, moved or deleted while it sounds, and across a tempo change. A new snapshot leaves a note it did not touch alone. The held list is fixed at 128 notes per track. One more is not played and is counted.
- The default project is a template of this extension: 120 bpm, 4/4, one arrangement with one track and its synth, no clips. Tone stays registered and is not part of it.

Agent-authored extensions remain supported and use the same SDK. They are a later capability, not the v0 headline. Codex subagents can keep running authoring tests against the SDK until the integrated agent exists.

### First milestone, decided September 19, 2026

Goal: a composer makes a short piece with tracks, clips, notes and one synth, an external agent adds a part by editing project files, and the composer sees and hears it live.

In scope:

- Core: the realtime engine from [ENGINEERING.md](ENGINEERING.md) section 3, the musical clock, transport, and the live project folder including records created and deleted from outside.
- Arrangement extension: tracks, clips and notes, an arrangement view and a note editor. No automation yet.
- Instrument extension: one subtractive synth. Tracks sum to the device output; there is no mixer yet.
- A transport control and the project menu from [DESIGN.md](DESIGN.md).
- A project agent doc that explains the folder layout and record formats, written together with the arrangement extension.

One process: the project runtime alone. The agent is an external coding agent such as Codex or Claude Code, run in the project folder. This already matches the rule that agents edit files and the runtime applies them live.

Decided September 19, 2026, for this milestone:

- Mouse editing covers adding, moving, resizing and deleting clips and notes, with a fixed 1/16 snap and scroll to zoom. Copy and paste, multi-select, a velocity lane and adjustable snap come later.
- Tempo changes are steps. Tempo ramps come later.
- The runtime starts with a project folder path and creates the default project when the folder is empty.
- Build in steps, one pull request each: workspace setup, realtime engine, clock and transport, live project folder, note contract and synth, arrangement with the agent doc, application window and views, then the milestone check.

Out of scope until later milestones: the outer application, agent sidebar, provider sign-in, in-app build and reload, project-local extension copies, mixer, sampler, effects, MIDI input, plugin host, recording and automation.

Second milestone, changed September 20, 2026: plugins, MIDI recording and fit tempo, see "Second milestone" below. The outer application, the agent sidebar and the agent integration are parked.

Verify the first milestone:

- An external agent with only file access adds a part in a bar range. The running project shows and plays it without a build or restart, and undo removes it.
- The agent adds a whole new track with its instrument by writing one new folder, during playback, without stopping the other tracks.
- The core crate contains no track, clip or note types. Tone from the lifecycle prototype runs on the same storage rule as a second, differently shaped tool.
- A generated project with 100 tracks of 100 clips opens, plays and applies a single clip edit live.
- Notes start at the expected frames for a given tempo, and a tempo change moves them accordingly.
- Closing and reopening the project restores the piece.
- The realtime checks from ENGINEERING.md pass on every `process` function.

#### Verified September 19, 2026

Checked on the real application on an Apple Silicon laptop: the window driven with real mouse and key events, and two outside agents, Claude Code with file tools only and Codex in its workspace-write sandbox. Each agent ran in the project folder and knew nothing but the generated `AGENTS.md`. Nobody listened: sound is proven by offline renders and counters, not by ear. The screenshots, logs and renders of the check are not in the repo.

| Item | Result | Evidence |
| --- | --- | --- |
| An agent adds a part in a bar range, live, and undo removes it | Pass | Prompt "Add a bass line in bars 5 to 8 that follows the chords on the piano track" during playback. Both agents wrote a bass track with a clip in bars 5 to 8, first try, `problems.txt` clean the whole time. Claude wrote three files 9 s apart, Codex in one burst. One cmd-z removed all of it from the window and the folder, one shift-cmd-z brought it back, and the render was byte-identical after that. The render differs only in bars 5 to 8. |
| An agent adds a whole track with one folder during playback | Pass | Prompt "Add a new track with a simple melody over bars 1 to 4". Same process before and after, 0 xruns, 0 late callbacks for the session. The audio claim is `live::a_track_folder_written_during_playback_adds_a_track_without_stopping_the_others` in `crates/runtime/tests/projects`: the other tracks are bit-identical. One undo, one redo. |
| No track, clip or note types in the core. Tone on the same storage rule | Pass | No type, function or constant in `crates/core/src` is named after a track, clip, note, pitch or velocity, and the core depends on no bundled crate. `state/drone.json` and two connections in `project.json`, written while the window was open, loaded with no problems: the render gained a sine of exactly the saved gain. Deleting the file removed its connections. |
| 100 tracks of 100 clips open, play and take an edit | Pass | 10,200 records. The window is on screen 1.1 s after the start, 2.8 s with a cold file cache. 35 s of playback with 0 xruns, slowest callback 1.4 ms. One outside clip edit showed within the next screenshot, 0.5 s later. Offline it applies in 0.2 ms (`scale::hundred_tracks_of_hundred_clips_open_play_and_take_an_edit`, run by hand). |
| Notes start on the expected frames, and a tempo change moves them | Pass | `timing::notes_start_and_end_on_the_frames_of_their_ticks`, `timing::another_tempo_moves_every_note`, `timing::a_tempo_change_during_playback_moves_the_notes_that_follow`, `held_notes::a_held_note_ends_on_its_tick_after_a_tempo_change` in the arrangement, and `quarter_notes_land_on_exact_engine_frames_for_every_device_buffer_size`, `a_tempo_change_moves_later_ticks_by_the_expected_frames`, `tick_to_frame_to_tick_is_exact` in the core. In the window, 60 bpm written into `project.json` during playback: the transport went from `2.3 0:03` of `0:16` to `2.4 0:07` of `0:32`, no problems, and cmd-z took it back. |
| Closing and reopening restores the piece | Pass | cmd-q, then the same command. The screenshot, the `--inspect` output, every file and a 16 s `--render` were byte-identical. `problems.txt` was gone after the quit. |
| The realtime checks pass on every `process` function | Pass | Three processors outside tests: `Tone`, `Synth`, `Sequencer`. Each runs under `RTSAN_ENABLE=1` in the tests of its crate, 224 tests. The check added the real-project tests of the runtime to that run. |
| The success sentence of CONCEPT.md | Pass, with an external agent | With the mouse in a new project: Add track, a clip by double click, three drawn notes, play, undo, redo, quit and reopen. Then the agent items above. |

Release build, first time: `cargo build --release -p runtime` takes 1.5 minutes and gives a binary of 12 MB. In the window the small project uses 2 % of a core stopped and 11 to 14 % playing, headless 1 % playing. The large one uses 8 % stopped and 20 % playing. 0 xruns in all. The dev build uses 9 % and 32 % on the large one. It also ran on a device at 96 kHz.

Found and fixed by the check: an undo could land on the middle of a drag (see "Editing and system services"), and the agent doc now tells an agent that writes and reads in one command to wait a second before it reads `problems.txt`. After that, Codex did.

### Second milestone, decided September 20, 2026

Goal: a composer records a piano take from a MIDI keyboard with no click, on a track that plays a third-party plugin. One action fits the project tempo to the take, so the grid follows the playing and the take sounds the same. A steadiness amount moves the tempo between as played and steady. Parts added after that, by hand or by an external agent, follow the take.

The order of work changed. A better DAW comes before an agent inside the product. The agent stays an external coding agent in the project folder, which the first milestone proved works. The in-app agent, the sidebar and the outer application are parked, not dropped.

The decisions, the scope, the steps and the checks are in [docs/milestone-2.md](docs/milestone-2.md). Each step records what it settles in this file.

#### Agent docs as a map, decided September 20, 2026 with step 0

One agent doc per project does not scale with the extensions and tasks of this milestone, so it is split. Built in `crates/core/src/project/generated.rs`.

- `AGENTS.md` is the map. It holds what an agent needs on every task: what the folder is, the layout with the form of every tool of the project, ids, ticks with the bar math of the project's time signature, the rules for writing a file, and how to check `problems.txt`. Then a table of the docs, one line each saying when to open it. `CLAUDE.md` still imports it. The map is about 960 words, half of the 2030 the one doc had.
- Every doc is a file in `agent-docs/` in the project folder, `<name>.md`. A folder, so the docs sit together and out of the way of the composer's own files, and `state/` and `project.json` stay the only places the runtime reads records from. The watcher ignores everything else, so the docs cause no outside change.
- A doc is an `AgentDoc`: a `name` (the file), a `when` (the one line in the map) and `markdown` starting with `# `. An extension registers one or more with `registry.agent_doc(EXTENSION, doc)`, one per task when a tool grows several, and the runtime registers docs every project gets with `registry.runtime_agent_doc(doc)`. The name becomes a file name, so it follows the same rule as an instance name, from the same function, and a name that does not is a registry error. Two docs of one name is a registry error too, because the second would write over the first. The core brings the doc of `project.json` itself, since `project.json` is core and not an extension, and `Registry::new` puts it in the list, so it takes its name like any other doc.
- The runtime owns the markdown of the folder and nothing else in it: a `.md` that no enabled extension registers is removed, so a doc never outlives its tools, and a file of any other kind next to the docs is the composer's and stays. Everything else stays as it was: written only when the text changes, nothing of the machine in them, a read-only open writes nothing. The placeholders of the project's time signature are filled in every doc, not only in the map.
- A test of the runtime loads every `json` example of the map and of every doc into one folder, as before.
- Checked with outside agents on September 20, 2026, see the step below. Both opened only the docs their task needed.

#### The terminal from the project menu, decided September 20, 2026 with step 0

The composer needs a terminal in the project folder to start a coding agent. "Open terminal in project folder" sits next to "Reveal project folder" in the project menu and runs `/usr/bin/open -a Terminal <folder>`: the system Terminal, which every Mac has. No picker, no setting, no terminal inside the window. Other platforms come when we claim them. The command is built by a function of its own so a test reads its program and arguments, which CI can do without a Terminal it cannot close. It runs on the background executor and a failure goes to the notice of the session.

#### Stereo signal path and the track mixer, decided September 20, 2026 with step 1

The path from an instrument to the device is stereo, and a track has a gain, a pan and a mute. Built in `crates/core/src/processor.rs` and `engine.rs`, and in `extensions/arrangement/src/mixer.rs`.

- One audio format everywhere: every audio port of the engine carries two channels, left then right (`sound_core::CHANNELS`). There is no mono port, so no processor, connection or device negotiates a channel count, and `project.json` connections keep the form they had. A processor that makes one signal, such as the synth, writes it into the left channel and copies that to the right. This was cheaper than a pair of named ports per signal: the note contract keeps its one `audio` port, a stereo cable cannot be half connected, and a plugin with two outputs in step 4 fits one port.
- `{"device_output": n}` is now the first device channel of the connection: the left channel goes to `n` and the right one to `n + 1`. A device that does not have that next channel plays the left channel alone. Nothing else changed in the saved form.
- A project of the first milestone connects one port once per device channel, which would now play it twice on the right. So a second connection of the same port to a channel the port already reaches stays saved, unused and reported, like a connection to a missing instance or one that closes a cycle. The problem names the line and says that one connection carries both channels. Such a project therefore sounds as it did, on both channels, and the composer or their agent is told what to remove. There is no migration and no format version: the runtime does not rewrite the file. Two different ports on channels next to each other are untouched; they sum, as they always did.
- Which of two overlapping lines wins is decided by channel, not by the order of the file: the lower channel carries both channels and the other line is the one reported. `project.json` connections are therefore all resolved before any of them reaches the graph, and what the graph holds and the new file does not keep is disconnected before the new lines connect. Without that, a file that lists its lines the other way round would play on the right channel alone, and removing the reported line from a running project would leave silence until the next open.
- Gain, pan and mute live in the `arrangement.track` record as `gain_db` (-60 to 6), `pan` (-1 to 1) and `mute`. A record that leaves them out is 0 dB, the middle and not muted, so a project of the first milestone opens, is not rewritten and plays exactly as it did. They are in the arrangement because they are the track, not a tool of their own; the bundled mixer extension of the v0 table is left for sends, buses and a mixer view.
- The pan law is equal power, scaled so that the middle is exactly 1 in both channels. So a centred track at 0 dB leaves every sample as its instrument made it, and an old project loses no level. Hard left or right, the channel that plays the track is √2, 3 dB above the middle, and the other is exactly 0. A track keeps its loudness wherever it is panned. There is still no limiter, so a hard-panned loud track has 3 dB less headroom than a centred one.
- Each track has one `Mixer` processor after its instrument. The behaviour works out the two channel gains on the control thread and sends them, so the audio thread computes no pan law, and a mixer edit is an update and no compile. The processor ramps to a new pair over 20 ms, so no change of gain, pan or mute clicks, and a muted track that has finished its fade returns before it touches its output.
- `Smoothed` moved from the synth into the core as the one smoothing helper of the SDK, which ENGINEERING.md section 3 already promised. The synth and the mixer use the same one.
- Each change is one undo step: a knob drag of the panel is one gesture, the mute button is one commit, a file edit is one group, as for any other record.
- Measured September 20, 2026, same laptop, dev profile: the project of 100 tracks with 100 clips plays 34 times faster than realtime offline, where it was 62 times in mono. Nearly all of that is the stereo buffers; the mixer of every track costs about 2 %. 100 synths holding a chord each still render 13 times faster than realtime, so a voice costs the same and the difference is the engine moving buffers. On the device, two tracks with five mixer edits written into their files while playing: 0 xruns, 0 late callbacks, slowest callback 236 µs at 44.1 kHz.
- Not built: solo, sends, buses, a master fader, meters, a limiter, a mixer view, mute on the track header, automation of these values, and a mono device that folds the two channels together.

### Agent context and tools

The agent works through the live project folder and the runtime protocol, not a separate edit API.

- Context: the project files, the enabled extensions' SDK docs and source, and the current transport position.
- Project tools: read and write records, which the runtime applies live. Transport operations and offline rendering through the protocol.
- Extension tools: edit extension source, build, reload.

The verification for v0 is one composition task, such as adding a part in a bar range, completed by an agent with only these tools and seen live by the composer without a build.

## Project storage

A project is a folder containing JSON state and separate assets. The project's text files together with its extensions fully describe the work; nothing needed to restore it lives outside them. This layout is illustrative:

```text
my-piece/
  project.json
  state/
    arrangement/              a root instance of the bundled arrangement tool
      instance.json           its record
      piano/                  a track: a child instance with children of its own
        instance.json
        instrument.json       the synth, a child with no children
        verse-a.json          a clip
        verse-b.json
    drone-machine.json        another tool, one small record
  assets/
    field-recording.wav
  extensions/
    custom-instrument/
  workspace.json
```

Decided September 19, 2026, the core storage rule. It says nothing about music:

- An instance is a folder under `state/` holding one JSON record. The record names its tool type.
- Owned child instances are subfolders. The folder tree is the ownership tree, so one parent per child and no cycles come for free. Deleting a folder deletes the instance and its children.
- The path is the stable ID. Folder names are readable and chosen at creation. Display names live inside the record, so renaming in an interface does not move the folder.
- Links that are not ownership, such as connections, sends or a shared clip, are saved references to a path. A reference resolves to an optional instance.
- The extension chooses how finely to split its state: one record or a deep tree.

Decided September 19, 2026, the file naming rule:

- The tool decides the form, for good. A tool that owns children says so in its code (`State::OWNS_CHILDREN`). Its instances are always a folder `<name>/` that holds the record as `instance.json` and the children next to it, also while there are no children. Instances of every other tool are always one file, `<name>.json`. There are no grouping folders: every folder under `state/` is an instance.
- The id of an instance is its path under `state/` without `.json`, for example `arrangement/piano/verse-a`. Names use lowercase letters, digits, `-` and `_`. `instance` is reserved.
- So adding a part is one new file, moving a clip to another track is moving a file, adding a track is one new folder, and deleting a folder deletes the instance and its children. What kind of child a file is comes from the tool name in its record, or from a name the owner gives meaning to, such as `instrument`.
- The runtime never moves a record between the two forms, so the path of a record never changes under an agent. The first build moved `piano.json` to `piano/instance.json` on the first child. An agent that then rewrote `piano.json` was ignored, and the stale file brought the instance back after a delete.
- What does not follow the rule is not loaded and is listed as a problem that names the right path: a record in the wrong form for its tool, both forms for one name, a child under a tool that owns no children, and a folder without `instance.json` with everything in it. Creating a child under such a tool from an interface is a typed error.
- A record is `{"tool": "<tool name>", "state": {...}}`. The runtime writes JSON with stable key order. A list or object of up to 100 characters stays on one line and longer ones get one line per item, so a clip has one note per line and a small edit is a small diff. Any valid JSON loads.

There is no instance index. The runtime finds instances by reading `state/`. `project.json` records the project format version, enabled extensions by name, the tempo map and core-owned connections:

```json
{
  "format": 1,
  "extensions": ["tone"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [
    {"from": {"instance": "tone-a", "port": "audio"}, "to": {"device_output": 0}},
    {"from": {"instance": "lfo", "port": "out"}, "to": {"input": {"instance": "filter", "port": "cutoff"}}}
  ]
}
```

Ports are named by the tool that exposes them. A change to tempo or connections from outside applies live. A change to `extensions` is refused while the project runs: the file is listed as a problem that says to reopen the project, and nothing of it applies until then. Turning an extension on or off means loading or unloading all records of its tools, which opening the project already does, and extensions change with a rebuild and a restart anyway. An invalid `project.json` stops the project from opening, so it is fixed before the runtime writes anything. Nothing else does: a record that does not load, a record whose behaviour fails and a connection that cannot be used are left out and listed as problems, and the rest opens.

The runtime writes `project.json` whole, so it takes care not to write over an outside change:

- Before an edit, undo or redo changes tempo or connections, the runtime reads `project.json`. If it changed on disk and the watcher has not delivered it yet, that change applies first, as its own undo step, and the edit lands on top.
- While `project.json` holds an outside change that did not load, the runtime does not write it. The edit still applies live, and the problem on `project.json` says that tempo and connection edits are not written. When the file is fixed it loads whole and is the later write, so those unwritten edits are gone. Undo can bring them back. Extensions have no version numbers: the code in the project's extensions folder is the version, and updating from a newer bundled copy is an explicit copy. Stable identifiers link records independently of display names.

Each extension defines its saved data using Rust types. The core normally handles serialization, writing and loading. JSON is the storage format; live extension code works with typed state.

The project folder is always live. The runtime writes each affected record atomically after an edit finishes, so the files on disk match the running project. The runtime also watches the folder: when a file changes underneath it, it applies the whole record as one typed action and one undo step, through the same path that interfaces use. No field-level diffing; if an interface edit and a file edit touch the same record at the same moment, the last one applied wins for that record. Agent edits and interface edits therefore share one path, including undo and view notifications. The runtime's own writes do not re-apply. There is no separate save step; versions come from git or explicit snapshots.

Extensions must be able to apply new state while running, not only at load. Loading is applying state from empty.

Records should stay small enough for an agent to read and rewrite cheaply, and projects must scale: 100 tracks with 100 clips each is an ordinary project, not a limit. Individual notes and parameters do not require separate files. The layout under the arrangement in the example above is that extension's choice, described in the v0 section; the core knows no tracks, clips or notes.

The watcher covers more than edits to existing records. New record files and folders, deleted ones, moved ones, and changes to `project.json` all apply live. File changes that arrive together are one undo step, so an agent request that touches eight clips undoes as one.

Decided September 19, 2026, how outside changes apply:

- The watcher only tells which paths changed. The runtime reads those paths again, compares them with the live project and applies the difference as one group. Event kinds are not used, so edits, new files and folders, deletions and moves all take one road, and tests call the same function with explicit paths. Loading a project is that road from an empty project.
- Grouping window: changes with less than 100 ms between them are one group and one undo step, labelled "File change". The group applies once the folder has been quiet for 100 ms, so that is also the delay before an outside change is heard. The runtime groups raw `notify` events itself. `notify-debouncer-full` emits per path on a timer, so a burst could be split across two of its batches.
- The runtime knows its own writes by a fingerprint of the bytes it last read or wrote per file. A file with the same fingerprint holds nothing new. A file that decodes to the state the project already has, for example with other spacing, is no change and no undo step, and the runtime does not write it back.
- A file that does not load leaves the live state unchanged, stays on disk and is listed as a problem with the path and the field, for example `state/tone-b.json: state.frequency_hz: invalid type: string "high", expected f32`. The rest of the group still applies. The same goes for records of unknown tools, which are never written or deleted. A problem goes away when the file loads or is gone.
- Creation and deletion are undoable from both sides. Undo of an outside creation deletes the files, and undo of an outside deletion writes them again with their connections.

Each record identifies its type. There are no schema versions; keeping code and saved data compatible is the composer's and their agent's responsibility. Extensions can register additional asset files for large or unusual data.

Importing a sample copies it into the project's assets by default. Saved references use that project-owned copy, so moving or deleting the original file does not break the project and its samples travel with it.

Each project keeps its own editable copies of the extensions it uses. Agent changes for one piece do not modify another project's extensions. Project templates supply starting copies; bringing later extension improvements into an existing project is an explicit update. Extension source travels with the project.

Preserve musical meaning, such as frequency ratios and rhythm groupings, in saved state. Keep workspace layout separate from musical content. Closing a view does not remove its instrument.

Missing extensions leave their saved data intact. A failed write must leave the previous file complete. The runtime writes each record to a temporary file and renames it into place, so every file is atomic on its own. An edit that touches several files writes them in dependency order: parent records before children, then connections in `project.json`. A crash between renames leaves a valid project with at most one stale record, which normal loading errors surface. Fully atomic multi-file commits are not required.

Compatibility between an extension's code and its saved data is the user's or their agent's responsibility. If a code change requires updating project files, they perform that update themselves. The core and SDK do not provide a migration framework or automatically run extension data conversions. Normal loading errors can be reported for diagnosis.

## Parameters, events and connections

The SDK provides typed parameters, typed events and sampled signal ports. Their exact Rust APIs remain open.

Parameter declarations describe their identity, value type, default and applicable range, unit and control mapping. They give agents enough information to discover parameters and allow basic controls to edit them. Extensions can provide custom interfaces.

A parameter's saved base value is distinct from its effective value during playback. Interfaces and agent actions can edit the base value. Scheduled changes and modulation can control playback without continuously rewriting saved values. Modulation connections and their settings are saved.

Extensions explicitly expose parameters for signal modulation and define how modulation combines with the base value. The SDK should provide helpers for addition, multiplication and smoothing. The core does not silently choose between meanings such as adding hertz and multiplying frequency by a ratio. Ordering and combination rules for multiple control sources remain to be specified.

Events carry precise timing. Extensions define their meaning and share Rust event types when they need to communicate. The core delivers events at sample positions without requiring a universal musical note representation.

Signal connections describe value format, channel layout and meaning. Matching numeric formats alone does not establish compatibility: an absolute frequency and a relative frequency adjustment are different contracts. The representation of these contracts remains open.

An oscillator, envelope and filter provide a reference example: audio flows through signal connections, an event triggers the envelope, and a saved cutoff parameter can receive explicit LFO modulation during playback.

## Audio engine, transport and time

One shared audio engine executes sound. The core owns transport operations and provides a general project timeline. Extensions use these facilities and cannot replace their core semantics. A bundled extension provides the transport interface; other extensions can provide controls or invoke the same operations.

Audio input and output device selection belong to the core. The engine supports live audio input and multiple output channels, making device channels available to tools through the SDK. Extensions define the musical use of those inputs and outputs.

MIDI device integration belongs to extensions. The core provides precise event timing; extensions connect MIDI keyboards and controllers to tools and define how to interpret their input. MIDI conventions are not required by the core.

Support feedback connections in the audio graph. Each feedback loop requires an explicit delay so execution order is defined. Tools may keep this routing internal. Delay-buffer APIs, minimum delay and processing granularity remain implementation decisions.

The timeline provides a common time coordinate and scheduling facilities for all tools. Decided September 14, 2026: the core timeline also owns a musical clock, meaning a tempo map and time signature, with conversion between bars and beats, seconds and samples. Every bundled tool and most agent requests refer to bars and beats, so one clock lives in the core rather than in each extension. Tracks and clips remain extension functionality, as do timeline views and arrangement workflows.

Projects can have a finite duration or run without a predetermined end. The runtime must not require preparing the entire duration before playback.

Extensions may still define other divisions of time or keep independent musical clocks. Their timing must ultimately translate into scheduling on the shared audio engine. The SDK provides the core clock's conversion facilities so extensions do this precisely.

Project position is distinct from the engine's advancing sample count. Audio processing can continue while project playback is paused or stopped, allowing live instruments and effect tails to continue.

Decided September 19, 2026: engine time is an integer frame count. Musical time in the core clock is an integer tick count, 960 ticks per quarter note. Bundled extensions save positions and lengths in ticks, not floats or seconds. Conversion from ticks to frames rounds in one place in the core, so results are repeatable. Extensions with other ideas of time can save their own format and convert through the clock.

Decided September 19, 2026, with the clock build. The details and reasons are in ENGINEERING.md section 3, "Musical clock".

- A tick lands on the frame that contains its exact time. Tempo is 10 to 1000 bpm in steps of 0.001 bpm. Within these bounds and from 16000 Hz up, tick to frame to tick is exact.
- The tempo map is a list of step changes at tick positions, with one time signature per project. It is the saved form in `project.json`: `{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}`. Invalid values fail to load with a reason.
- Processors get, each block, the half-open range of ticks the block covers and the exact frame offset of any tick in it. Blocks cover the timeline without gaps or overlaps, so extensions never round time themselves.
- A tempo map change during playback keeps the position in ticks. The position in frames and seconds changes with it.

### Transport operations

| Operation | Core behaviour |
| --- | --- |
| Play | Advance from the current project position, including resuming after pause. |
| Pause | Hold the current project position and suspend timeline-driven playback. |
| Stop | End timeline-driven playback and return project position to zero. |
| Seek | Move to a chosen project position without changing whether playback is running. |

The core handles project position, invalidates scheduled events that no longer apply and notifies tools of transport changes. Tools define and document their sound response, such as releasing voices, resetting a pattern or allowing an effect tail to finish.

Seeking does not replay every event between the previous position and the destination. The core notifies tools of the position change and provides the context needed to schedule from the destination. Extensions define the musical response; the core does not prescribe note or clip behaviour.

Exact reconstruction of stateful audio at a seek destination is a separate capability, not a guarantee of seeking.

Decided September 19, 2026: the notification is two flags in the process context, each set for one block. One says the position jumped, after a seek or a stop. The other says playback stopped, after a pause or a stop. Tools release what they hold on either. The core holds no scheduled events. Timeline-driven processors make their events block by block from the transport info, so after a seek there is nothing to invalidate. The playhead and the playing state reach the control side through the engine status.

## Editing and system services

The core implements project file writing and watching, undo/redo and notification delivery. The SDK exposes these services; extension authors choose when to call them and how they fit their tool's workflow.

Interfaces and agents use the same typed editing actions. Authors choose meaningful edit boundaries and can group multiple actions into one undo step. The SDK records affected project state and handles restoration, so authors do not need to implement the reverse of each state edit.

Agents and composers can edit project state concurrently through existing actions without rebuilding. Conflicting writes use last-write-wins semantics, ordered by application in the runtime. Stale edits do not require conflict rejection or agent reapproval. An outside file edit applies immediately without ending an active interface drag. Later drag updates may overwrite the file edit. Undo and redo also apply as later writes and may overwrite intervening changes. A cancelled drag goes back to what the file edit wrote. Last write wins throughout; no special conflict handling is required.

For a drag gesture, begin an edit, publish updates during the drag, then finish it as one named undo step. Cancellation restores the original state. Playback and other views can respond to published updates before the gesture finishes.

Publishing a state edit through the SDK automatically notifies affected views. User-facing messages remain an explicit extension choice. Finishing an edit writes the affected records to the project folder; updates published during a gesture do not.

Undo and redo history are session-only and reset when the project closes or the runtime reloads. Current musical content and settings survive through the project folder; persisting edit history is not required.

Provide recommended patterns and the underlying operations for custom workflows. Overlapping edits follow the same last-write-wins rule.

Decided September 19, 2026, the edit API and undo, built in `crates/core/src/project`:

- One state application takes a group of changes: set a whole record (which also creates), delete an instance with everything it owns, and change tempo or connections. Interface edits, file changes, loading, undo, redo and cancel all call it. It applies the group whole, as one engine batch, or not at all. It notes the record before and after, which is all undo needs.
- An edit is `begin`, any number of `publish` calls with a group of changes, then `finish` or `cancel`. It may touch many records, create and delete. `publish` applies live and writes nothing. `finish` writes every touched record once and adds one undo step, from the committed state before it to the live state at the finish, whoever wrote last. `cancel` applies the committed states through the same path. The committed state of a record is its state before the first publish, or what a file change, an undo or a redo wrote during the edit. So no undo step holds the middle of a gesture: a file change during a drag undoes to the state before the drag, the drag undoes to what the file change wrote, and a cancel leaves what the file holds. Built with the milestone check. Before, the file change took the state in the middle of the drag as its before side. Not covered: tempo and connections, which no gesture edits yet. Several edits may be open at once.
- Undo history is two stacks of steps. A step holds the before and after record of every instance it touched, shared with the live state and not copied. Undo applies the before side as one group and writes the files. A step that can no longer apply is dropped with a typed error, for example because its owner is gone, or because a file the runtime did not load now sits at the id of an instance that undo would write.
- Writing: parents before children, then deleted records, then `project.json`. Each file is a temporary file renamed into place, with no `fsync`: it cost 6 ms per file on macOS, which made undo of a deleted folder of 100 records block for 0.7 s. The rename protects against a crash of the process. Surviving a power loss is left to git and snapshots. A failed write leaves the old file complete, the edit stays live and undoable, and the problem is listed until a later write succeeds. The runtime only removes record files it knows, so deleting an instance never removes records of unknown tools or other files in its folder.
- The UI layer drains a list of small events after each call: created, changed, deleted with the instance id, project file changed, problems changed. Events carry no state.
- A new project has no undo history: making its default content is not a step. The runtime makes the default project when the folder has no `project.json` and no records, whatever else is in it, such as `.git`.
- A tool may register a summary: a function from the project and one instance to lines of text about it and what it owns. The core knows no tracks, so this is how `--inspect` tells what plays where. An instance without one is listed as its record.
- The runtime writes the generated files into the project folder, each only when its text changes. `AGENTS.md` is the map for an agent, `CLAUDE.md` imports it, `agent-docs/*.md` are the docs the map lists, and `problems.txt` lists what is not live, one line per problem, and is absent when there is none. A read-only open writes nothing. See "Agent docs as a map" below for what is in the map and what is in a doc.
- Undo of outside changes: groups of outside changes that follow each other within 15 s (`OUTSIDE_UNDO_WINDOW`), with no interface edit, undo or redo in between, are one undo step. The step keeps its oldest before side and takes the newest after side, and a step that ends where it began is dropped. The 100 ms quiet window still decides when a change is heard. Reason: two runs with an external agent showed that it writes the files of one request seconds apart, so a new track was two or three undo steps. This is a heuristic. The second milestone replaces it with the real boundaries of an agent request.
- `problems.txt` is there the whole time a runtime has the project open with its lock, with the line `No problems. Every file is live.` when there are none, and the runtime removes it on a clean close. So for an agent a missing file means that no runtime is watching: its edits are saved and unchecked. A file left by a crash can be stale, and the agent doc says so.
- A tool may say where its instances live (`State::PLACE`): anywhere, only at the top of `state/`, or only directly inside an instance of one named tool. A record somewhere else is not loaded and is listed as a problem that says where it belongs. The arrangement uses it, so a clip outside a track or a track outside the arrangement is a signal to the agent and not silence. The core still knows no tool by name.
- The generated agent doc holds nothing of the machine, such as the path of the runtime, so a project in git gets no diff from being opened by another build.
- One runtime per project: the runtime holds a lock on `.sound-tools.lock` in the project folder. Inspecting and offline rendering open the project read-only without the lock, so they work next to a running runtime.

### The window and its views, decided September 19, 2026

Built in `crates/ui` (the bridge), `extensions/arrangement/src/view.rs` and `crates/runtime/src/window.rs`. The guide for view authors is [crates/ui/README.md](crates/ui/README.md).

- One process, one window, one project. `runtime <folder>` opens the window. `--headless`, `--inspect` and `--render` start no GPUI.
- The bridge is one GPUI entity, `Session`, in the UI SDK. It owns the `Project` on the main thread, polls the engine and the watcher every 16 ms from a timer, emits every `ProjectEvent` and notifies once per group. The UI SDK therefore depends on the core. The core stays free of GPUI.
- The playhead is an entity of its own. It changes on every frame during playback, and only what shows the position observes it. GPUI renders a notified view and every view above it, so the arrangement keeps its playhead line beside its timeline, not inside it, and the timeline is a cached view. While playing, a frame does not run the timeline code at all.
- Views hold the session and typed instances, read state when they render and keep no copy of saved state. The one exception is what costs a walk over many records: the arrangement keeps its track order and its end, and the transport the end of the project, between the project events that can change them, and reads them again once per group of events. Every edit goes through `Session::edit`, which puts an error into one notice that the window shows as a quiet line. Files that are not live show as a second line that names `problems.txt`.
- Extensions register a view per tool in a small registry (`Views`). The main area of the window shows the view of the first instance at the top of the project whose tool has one. This is provisional: workspace composition is second-milestone work. The window names no arrangement type, with one exception: "Add track" calls the arrangement helper with the default synth, next to the default project, which knows the same.
- A tool may say where its content ends (`ToolRegistration::end`), and `Project::end` is the latest of them. The transport shows it as the duration and the length of its seek strip. The arrangement gives the end of its last clip. The core still knows no clips. Playback does not stop at the end.
- The arrangement view paints on one canvas and only what is visible: no element per clip. All coordinate math (tick to x, track to y, snap to a sixteenth, visible range, zoom about the pointer, the miniature of the notes) is pure functions with tests in `view/layout.rs`. Mouse listeners are registered while painting and get the scene that was painted, so a click hits what is on screen. Zoom, scroll and selection are interface state and are not saved.
- The note editor is a panel inside the arrangement view, owned by the arrangement extension. The main area of the window stays one root view, and the window does not know the editor. It opens for the selected clip, follows the selection to another clip, and closes when its clip is deleted, from inside or outside.
- One detail area below the arrangement, one thing at a time, decided September 20, 2026. It shows the note editor of a clip or the track panel of a track, like the clip view and the device view of Ableton. A double click or enter on a clip opens the editor, a click on a track header opens the track panel, and each takes the place of the other. Escape or the close control closes either. Both have one height. The selected track is interface state of the timeline, like the selected clip.
- The track panel belongs to the arrangement and is about the track, not about a synth. A track owns clips and one child named `instrument`, and any tool with the ports of the note contract fits there. The panel is a rack of device cards, left to right. For each slot it asks the view registry for the view of whatever instance is there and hosts it in a card, so the arrangement still depends on no instrument, and the controls of the synth live in the instrument extension, which registers a view for `instrument.synth`. A slot that is empty or whose tool has no view shows a quiet card with the tool name. Today the rack holds the instrument alone. Since step 1 of the second milestone the gain, pan and mute of the track are a fixed section at the right end, after the rack. Effects become more slots in the rack; they are not built and there is no placeholder for them.
- The view registry is a GPUI global (`Views::view_of` from any view). `Shell::new` takes the `Views` and installs it, so the contract is still in a type and a caller cannot forget it. It was a field of the window before, which a nested view could not reach. A global is the smallest change: one process has one window and one project, nothing has to be passed down through view constructors, and a host needs no type of what it hosts.
- The synth view is the first view on a record with plain parameters. Its knob is a controlled component: the view gives the value on every render and the knob reports changes, so the view keeps no copy of saved state and an outside edit shows at once, also during a drag. A knob drag is a session gesture that begins with the first change. The ranges and the defaults of the synth are written once, as `Parameter` constants next to `SynthState`, and `validate`, the knobs, the reset and a test of the docs read them. This is local to the instrument crate. The declarative parameter system below stays open: one tool is not enough to design it from.
- Measured September 20, 2026, same laptop, dev profile: GPUI in the instrument crate costs the DSP loop about 0.1 s. `cargo build -p runtime` after a touch of `synth.rs` takes 1.85 s (1.94, 1.85, 1.84), 1.7 s before. After a touch of the synth view file 1.75 s, of the track panel file 1.8 s. `cargo nextest run -p instrument` after a touch of `synth.rs` takes 2.1 s, as before, and the instrument crate alone compiles in 0.57 s, 0.47 s without the view module. So the view stays in the crate of its tool.
- The track panel was checked in the real window on September 20, 2026, with real mouse and key events on a device at 96 kHz, while a note of two pitches played: a click on the track header opened the panel, a drag of the cutoff knob wrote `"cutoff_hz": 150.0` once at the end, cmd-z and shift-cmd-z put it back and again, a double click set 2000, a file edit from outside showed in the knobs within a second, a click switched the waveform, and escape closed the panel. 0 xruns, 0 late callbacks, slowest callback 0.04 ms. Nobody listened. Escape during a knob drag and the tab order are proven by the simulated tests only: the tool that posts the events cannot hold a drag.
- A drag is a gesture of the session: `begin_gesture`, `gesture` per mouse move, `finish_gesture` or `cancel_gesture`. The session keeps the open edit, so it knows a drag is going on, and `Session::undo` and `Session::redo` do nothing until it ends. The window's keys and menu call these two. Undo in the middle of a drag would be overwritten by the next mouse move. One gesture at a time: a new one finishes one that was left open.
- Editing in the arrangement and the note editor, built September 19, 2026. The rules are in `extensions/arrangement/README.md`, "Editing rules". What the docs did not decide before:
  - A drag begins its gesture with the first mouse move that changes something, so a click is no undo step. It moves by whole snap steps from where it began and does not snap the result, so what an agent wrote off the grid keeps its offset.
  - The left edge of a clip stops at its first note. It keeps the notes at their project positions and drops none. The right edge drops notes that start outside, as `Clip::set_length` says, and undo brings them back.
  - A drag to another track is a delete and a create per mouse move. A drag that comes back to its first track takes its first id again, so the file stays where it was.
  - When the dragged clip is deleted from outside, the gesture finishes and does not cancel: the delete was the last write, and a cancel would bring the clip back over it.
  - The note editor puts the notes of a clip in order by start and pitch when a note edit ends, as the agent doc asks of an agent.
  - The selected note is kept by value, not by index, and a clip resize goes on from the live clip when something else wrote it. Both for the same reason: the clip changes under an open view, and a view must never edit what only took the place of what the composer chose.
  - During an open gesture the transport does not read the end of the project again, and the arrangement reads only the track of the changed clip. Both walked every clip per mouse move before. The duration follows when the drag ends.
- The preview note: a note that is clicked, drawn or moved to a new pitch sounds for 0.3 s through the instrument of its track, also while the project is stopped. The core got one call for it, `Project::send`: an update from an interface to a processor of an instance, outside any edit. The sequencer ends the note by itself in engine time, so no interface can leave one sounding.
- The keys of a view are key listeners on its focused root, not bindings: delete, the arrows, enter and escape reach the arrangement or the editor only while it has the focus. The bindings of the window run first, so space still plays. The focus ring of these two views shows only when the focus came from the keyboard. GPUI's focus-visible also shows it when a key follows a click, which space does all the time.
- The keys of the window are bound in its key context, and space, cmd-z and shift-cmd-z only outside a text field, so a text field gets a space and its own undo later. When the focused control goes away, the focus returns to the window root, so the keys keep working.
- A notice knows where it came from. A good edit clears the notice of a failed edit only. One from the engine, the watcher or the device is reported once and stays until it is dismissed.
- Switching the output device is out of scope for the first milestone: it needs a stream restart and a new prepare at another sample rate. The project menu shows the current device by name only.
- Not shown anywhere yet: the tempo. It did not fit the transport under the quiet rule. It comes with tempo editing.
- Quitting: macOS ends the process without unwinding. GPUI drops the window and its views first, so every strong handle to the session lives in a view and the key bindings hold it weakly. The project is then dropped, which removes `problems.txt`, as a clean close must.
- The repaint issue of the lifecycle prototype was checked in the real window on macOS 26 with the pinned GPUI. A file written from outside shows within one poll while the window is visible and another application has the focus. After hiding the application, deleting a track folder and showing it again without giving it the focus, the window showed the new state. So no workaround is needed. An occluded window may still stop drawing, which is fine: it draws when it is seen again.
- Measured September 19, 2026 on the same laptop and project, with real mouse events in the headless window: one mouse move of a clip drag, with its publish, every project event and the frame after it, takes 1.8 ms, across tracks 1.9 ms, and one move of a note drag 1.9 ms. They took 3.5 to 4.1 ms before the two fixes above. The publish alone, with the behaviour of the track and a new snapshot of its 100 clips, takes 0.02 ms. `cargo build -p runtime` after a touch of the note editor file takes 1.6 s (2.0, 1.6, 1.6).
- Measured on an Apple Silicon laptop, dev profile. A frame of the window (one update and its `Window::draw`, without the GPU) on the project of 100 tracks with 100 clips: 1.4 ms while playing, 1.7 ms while scrolling, 3.6 ms while scrolling zoomed far out with every clip of the visible tracks on screen. Reading the end of that project takes 0.9 ms, after the walk over the children of an instance was changed to step from child to child (it looked each child up before, 2.7 ms). The transport reads it once per change, never per frame. CPU of the visible window: about 3 % stopped, of which 1.6 % is what any GPUI window uses and 0.7 % the engine and the watcher, and about 20 % while playing two tracks, with GPUI unoptimized. `cargo build -p runtime` after a touch of an arrangement source file went from 0.6 s to 1.6 s. 1.2 s of that is linking GPUI into the runtime, which the window needs wherever its code lives. The view module in the arrangement crate costs about 0.2 s, so it stays there.

## Next decisions

[The SDK sketch](SDK_SKETCH.md) proposes a Tone lifecycle and a small next prototype. Its APIs remain provisional. Last write wins for file edits, active drags, undo, redo and cancellation; do not reopen this as a synchronization problem.

The first milestone is built and was verified on September 19, 2026, see "Verified September 19, 2026" under it. Its steps, with what each left out:

- Done September 19, 2026: the realtime engine with device output and the control-to-audio handoff, following ENGINEERING.md section 3. Tone plays through it from `extensions/tone`. Feedback connections, audio input and device selection are not built yet.
- Done September 19, 2026: the musical clock and the transport in `crates/core`. Loop playback, tempo ramps and time signature changes are not built yet.
- Done September 19, 2026: the live project folder in `crates/core`, with tool registration, owned children, references, editing with undo, storage, the watcher and the engine binding. Tone is the first tool on it. `cargo run -p runtime -- <folder>` runs a project headless. Not built: reacting to referenced instances, declarative parameters, assets.
- Done September 19, 2026: the instrument and the arrangement extensions, the default project, the project summary and the project agent doc. Not built: automation, mixer, mute and solo, loop playback.
- Done September 19, 2026: the application window with the session bridge, the view registry, the arrangement view, the transport and the project menu. Not built: following the playhead when it leaves the view, device switching, a tempo display.
- Done September 19, 2026: editing clips and notes in the window with the mouse and the keys, the note editor and the preview note. Not built: copy and paste, multi-select, a velocity lane, adjustable snap, splitting clips, selecting a clip with the keys alone, renaming tracks.
- Done September 19, 2026: the milestone check on the real application, with two outside agents, a release build and the root `README.md`.
- Done September 20, 2026, after the milestone: the track panel with the view of the synth, see "The window and its views". Not built: effects, the mixer section of a track, an instrument picker, reordering devices.

Second milestone steps:

- Done September 20, 2026, step 0: the agent docs as a map with one doc per extension, and the terminal from the project menu. See "Agent docs as a map" and "The terminal from the project menu". Not built: a doc per task (no task needs one yet), other platforms than macOS for the terminal.
- Done September 20, 2026, step 1: the stereo signal path and the gain, pan and mute of a track, in the record, in the track panel and from a file. See "Stereo signal path and the track mixer". Not built: solo, sends, buses, a master fader, meters, a limiter, a mixer view and automation.

The repaint issue from the lifecycle prototype is understood: macOS stops rendering an occluded window. It was re-checked in the real window and needs no workaround, see "The window and its views". The pinned GPUI has an accessibility tree and focus-visible. The menu trigger and the seek strip use focus-visible; the other components and the accessibility tree are open.

What the second milestone starts from: one process, the project runtime, that opens a project folder in a window or headless, keeps it live in both directions and plays it. An external agent already composes in it through files alone, with `AGENTS.md` as its context, `problems.txt` as its check and one undo step per request by the 15 s rule. The second milestone keeps that and makes the DAW worth using: stereo tracks with gain, pan and mute, MIDI recording, CLAP and VST3 plugins, and fitting the tempo to a free take. The plan is [docs/milestone-2.md](docs/milestone-2.md).

Open:

- Declarative parameter metadata and generic parameter controls.
- Agent integration, the outer application/runtime protocol details and window/workspace composition. Parked September 20, 2026, until the DAW itself is further along. The 15 s rule for one undo step per agent request stays until then.
- Whether the agent gets the transport and `--inspect` as tools. No agent has used `--inspect` yet: Claude Code ran with file tools only, and Codex had a shell but no `runtime` on its `PATH`. Both managed without it.

### Known gaps after the first milestone

Collected from every step and the check. Decided limits are in the sections above and are not repeated here.

Sound and engine:

- Nobody has listened with care. All proof of sound is counters, offline renders and sample comparisons. The synth defaults need an ear.
- No limiter. Tracks add up, and one square note at full resonance and velocity can peak above 1.0. Since the second milestone a track has a gain, a pan and a mute, and nothing else of a mixer.
- No feedback connections, no audio input, no device switching, no new `prepare` after a sample rate change. Only f32 output on the default device, only macOS.
- No App Nap prevention. A long session in a hidden window is not tried.
- A routing edit is a hard switch, without a gain ramp. Tone steps its gain. The synth smooths its own. There is no smoothing helper in the SDK.
- Aliasing of the synth is not measured. Speed on x86 is not measured. No Miri run.

Time and transport:

- Playback does not stop at the end of the project and there is no loop. During an agent request the playhead runs far past the piece.
- The view does not follow the playhead. The tempo shows nowhere in the window.
- No tempo ramps, no time signature changes, no chase of notes on a seek.

Project folder and undo:

- The 15 s rule for one undo step per agent request is a heuristic. It held for both agents, with up to 5 s between two files. A slower agent gets two steps.
- `problems.txt` does not say which write it has seen. An agent that reads it in the same command as its write can see the old text. The agent doc now says to wait a second.
- A `problems.txt` left by a crash can be stale.
- An undo step for tempo or connections can still hold the middle of a gesture. No gesture edits them yet.
- A file edit of a clip while that clip is dragged across tracks comes back as a second clip.
- A behaviour does not react to an instance it only references. Port names in `project.json` are strings without a check at build time.
- No assets, no `workspace.json`, no declarative parameters. No `fsync`, by decision. The file watcher is tried on macOS only.
- The stdin commands of `--headless` are provisional. They are not the protocol of the outer application.

Window:

- CPU: the release build uses 11 to 14 % of a core while it plays a small project in the window, and 1 % headless. So nearly all of it is the window, which draws a frame for the playhead at the rate of the display. The dev build uses 20 % and more.
- Not tried by hand: pinch zoom on a trackpad, the resize cursor on a real screen, dragging the window by its top row.
- GPUI's focus-visible covers the menu trigger and the seek strip. The arrangement, the note editor, the knob and the segmented control show their ring for a focus from the keyboard only. The button still shows it on any focus. The accessibility tree is not used.
- The track panel: nobody has turned a knob by ear yet, only by event injection and offline renders. The rack scrolls sideways with the wheel or the trackpad, but a knob that tab reaches outside the visible part is not scrolled into view.
- Every change of an instrument runs the behaviour of its track again, which builds a new `TrackSnapshot`: the same cost per mouse move as a clip drag. Fine until a profile says otherwise. A knob has no fine drag with a modifier and no typed value. Track selection has no keys of its own while a clip is selected: click a header, or deselect the clip.
- The main area shows the first top instance with a view, and "Add track" names the arrangement. Both go with workspace composition.
- The timeline and the note editor each have their own block of mouse listeners.

Tooling:

- CI runs on macOS only. The list of crates for the realtime sanitizer is kept by hand.
- A build after a touch of a view file takes 1.6 to 2 s, most of it linking GPUI.

Reference code: [pi-mono](https://github.com/badlogic/pi-mono) for extension registration and agent access to docs, and [Pure Data](https://github.com/pure-data/pure-data) for processor composition and scheduling. Neither dictates the product's UI or musical model. [ENGINEERING.md](ENGINEERING.md) records tooling, dependency and audio engine recommendations drawn from Zed, Pure Data and Elementary.

## Verification

- An agent with only file access and the runtime protocol completes a composition task on the bundled arrangement, such as adding a part in a bar range, and the composer sees and hears it live without a build.
- Bars and beats from the core clock convert exactly to samples, and a tempo change moves scheduled events accordingly.
- An external agent builds a custom GPUI editor from SDK docs and examples, and embeds it twice with independent state.
- A composite tool reuses child tools, exposes selected ports and restores their state without duplication. An alternative view edits the same musical content.
- Editing one extension reuses unchanged build dependencies; measure build and restart time.
- Reload restores musical content and settings. Compilation failure leaves the previous executable usable.
- Reloading the project runtime preserves the outer application's agent conversation. An external agent can inspect, edit and render a project without opening graphical interfaces by reading and editing project files.
- Parameters change during playback without rebuilding.
- A drag updates playback and shared views, creates one undo step, and can be cancelled. Undo/redo restores grouped project edits made through either an interface or the agent.
- Modulation changes the effective parameter value while preserving its saved base value; reopening restores modulation connections and settings.
- Compatible event and signal ports connect, incompatible contracts produce useful errors, and scheduled events reach processors at the intended sample positions.
- Independent musical clocks schedule precisely on the shared engine. Core transport controls apply consistently, and open-ended projects do not require preparing their entire duration.
- Pause holds position, play resumes, stop returns to zero, and seeking preserves whether playback is running. Seeking clears obsolete scheduled events without replaying skipped events and notifies tools of the position change.
- Reopening restores instances, connections, assets and extension-defined musical data. Missing extensions and failed writes preserve existing work.
- Editing a project file on disk applies live in the running project and creates an undo step. A finished interface edit appears on disk, and the runtime does not re-apply its own writes.
