# Technical architecture

This document records architecture decisions and proposals. The product goals are in [CONCEPT.md](CONCEPT.md). Revised September 14, 2026: the v0 is a small DAW with an agent sidebar, built from bundled extensions on a small core. Revised September 19, 2026: the first milestone is cut down and the saved time format, record size and watcher scope are decided. The small GPUI build-loop experiment below has been validated on macOS. An isolated core lifecycle prototype now implements typed state, persistence, editing, offline processing and GPUI views for Tone and an agent-authored Tremolo. The full application remains unimplemented.

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

The project manages tool instances. A composite tool can explicitly own child tool instances; deleting the parent also deletes those children. Connections and references do not establish ownership, so deleting a connected tool does not delete its peers. Views reference musical content without owning a second copy, and closing a view does not delete that content. The exact APIs for registration, ownership and shared references remain open.

Audio plugin hosting belongs entirely to extensions. A bundled plugin host extension provides it for the v0 DAW; the core has no plugin interface. The hosting design is not specified here.

## v0: DAW workspace from bundled extensions

The v0 workspace is a small DAW. Its parts are bundled extensions that ship with the product and are part of the default project template. They use the public SDK only; nothing in them is a core feature.

| Bundled extension | Provides |
| --- | --- |
| Arrangement | Tracks, clips, notes and automation on the core timeline, with an arrangement view and a clip or note editor. |
| Mixer | Track levels, pan, sends and the mixer view. |
| Instrument | One subtractive synth with a few parameters. |
| Sampler | Plays imported samples from project assets. |
| Effects | Two or three, such as delay, filter and reverb. |
| MIDI input | Maps MIDI devices to instrument tracks. |
| Plugin host | Loads third-party audio plugins (VST3, AU, CLAP) as instruments and effects on tracks. |

Build the core and these extensions together. Each extension should be small and finished before starting the next. Order: arrangement and instrument first, since they prove the note contract, the musical clock and live agent edits. Plugin host last, since it depends on the note and audio contracts being stable.

The arrangement saves a folder per track and a file per clip. A track record holds its name, colour and order. A clip record holds its own start, length and notes, one note per line, so adding a part is one new file and moving a clip to another track is moving a file. A track owns its instrument as a child instance and goes to the main output by default, so adding a track is one new folder and no `project.json` edit. The headless inspect command prints a project summary, so agents do not need to open every clip to answer what plays in a bar range.

The arrangement extension's saved format becomes the de facto note and clip contract other extensions read. It lives in a bundled contract crate, not in the core. The core stays independent of notes, tracks and clips.

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

Out of scope until later milestones: the outer application, agent sidebar, provider sign-in, in-app build and reload, project-local extension copies, mixer, sampler, effects, MIDI input, plugin host, recording and automation.

Second milestone: the outer application, the agent sidebar and the agent integration. After that, the remaining bundled extensions in the order above.

Verify the first milestone:

- An external agent with only file access adds a part in a bar range. The running project shows and plays it without a build or restart, and undo removes it.
- The agent adds a whole new track with its instrument by writing one new folder, during playback, without stopping the other tracks.
- The core crate contains no track, clip or note types. Tone from the lifecycle prototype runs on the same storage rule as a second, differently shaped tool.
- A generated project with 100 tracks of 100 clips opens, plays and applies a single clip edit live.
- Notes start at the expected frames for a given tempo, and a tempo change moves them accordingly.
- Closing and reopening the project restores the piece.
- The realtime checks from ENGINEERING.md pass on every `process` function.

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
      arrangement.json
      tracks/
        piano/
          track.json
          instrument/
            synth.json
          clips/
            verse-a.json
            verse-b.json
    drone-machine/            another tool, one small record
      drone-machine.json
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
- The extension chooses how finely to split its state: one record or a deep tree. Exact file naming is settled when building.

There is no instance index. The runtime finds instances by reading `state/`. `project.json` records the project format version, enabled extensions by name, the tempo map and core-owned connections. Extensions have no version numbers: the code in the project's extensions folder is the version, and updating from a newer bundled copy is an explicit copy. Stable identifiers link records independently of display names.

Each extension defines its saved data using Rust types. The core normally handles serialization, writing and loading. JSON is the storage format; live extension code works with typed state.

The project folder is always live. The runtime writes each affected record atomically after an edit finishes, so the files on disk match the running project. The runtime also watches the folder: when a file changes underneath it, it applies the whole record as one typed action and one undo step, through the same path that interfaces use. No field-level diffing; if an interface edit and a file edit touch the same record at the same moment, the last one applied wins for that record. Agent edits and interface edits therefore share one path, including undo and view notifications. The runtime's own writes do not re-apply. There is no separate save step; versions come from git or explicit snapshots.

Extensions must be able to apply new state while running, not only at load. Loading is applying state from empty.

Records should stay small enough for an agent to read and rewrite cheaply, and projects must scale: 100 tracks with 100 clips each is an ordinary project, not a limit. Individual notes and parameters do not require separate files. The layout under the arrangement in the example above is that extension's choice, described in the v0 section; the core knows no tracks, clips or notes.

The watcher covers more than edits to existing records. New record files and folders, deleted ones, and connection changes in `project.json` all apply live. File changes that arrive together are one undo step, so an agent request that touches eight clips undoes as one. The lifecycle prototype only handled edits to existing records.

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

### Transport operations

| Operation | Core behaviour |
| --- | --- |
| Play | Advance from the current project position, including resuming after pause. |
| Pause | Hold the current project position and suspend timeline-driven playback. |
| Stop | End timeline-driven playback and return project position to zero. |
| Seek | Move to a chosen project position without changing whether playback is running. |

The core handles project position, invalidates scheduled events that no longer apply and notifies tools of transport changes. Tools define and document their sound response, such as releasing voices, resetting a pattern or allowing an effect tail to finish.

Seeking does not replay every event between the previous position and the destination. The core notifies tools of the position change and provides the context needed to schedule from the destination. Extensions define the musical response; the core does not prescribe note or clip behaviour.

Exact reconstruction of stateful audio at a seek destination is a separate capability, not a guarantee of seeking. Transport notification types and event invalidation mechanics remain to be designed.

## Editing and system services

The core implements project file writing and watching, undo/redo and notification delivery. The SDK exposes these services; extension authors choose when to call them and how they fit their tool's workflow.

Interfaces and agents use the same typed editing actions. Authors choose meaningful edit boundaries and can group multiple actions into one undo step. The SDK records affected project state and handles restoration, so authors do not need to implement the reverse of each state edit.

Agents and composers can edit project state concurrently through existing actions without rebuilding. Conflicting writes use last-write-wins semantics, ordered by application in the runtime. Stale edits do not require conflict rejection or agent reapproval. An outside file edit applies immediately without ending an active interface drag. Later drag updates may overwrite the file edit. Undo, redo and cancellation also apply as later writes and may overwrite intervening changes. Last write wins throughout; no special conflict handling is required.

For a drag gesture, begin an edit, publish updates during the drag, then finish it as one named undo step. Cancellation restores the original state. Playback and other views can respond to published updates before the gesture finishes.

Publishing a state edit through the SDK automatically notifies affected views. User-facing messages remain an explicit extension choice. Finishing an edit writes the affected records to the project folder; updates published during a gesture do not.

Undo and redo history are session-only and reset when the project closes or the runtime reloads. Current musical content and settings survive through the project folder; persisting edit history is not required.

Provide recommended patterns and the underlying operations for custom workflows. The exact edit API and undo storage mechanism remain implementation choices. Overlapping edits follow the same last-write-wins rule.

## Next decisions

[The SDK sketch](SDK_SKETCH.md) proposes a Tone lifecycle and a small next prototype. Its APIs remain provisional. Last write wins for file edits, active drags, undo, redo and cancellation; do not reopen this as a synchronization problem.

Immediate next work is the first milestone above, in this order:

- The realtime engine with device output and the control-to-audio handoff, following ENGINEERING.md section 3. Offline rendering works in the prototype; realtime audio does not exist yet.
- The musical clock in the core timeline.
- The live project folder with external record creation and deletion.
- The arrangement and instrument extensions on top, with the project agent doc.

The repaint issue from the lifecycle prototype is understood: macOS stops rendering an occluded window. Re-check it once in the real application. The pinned GPUI has an accessibility tree and focus-visible; the UI components do not use them yet.

Open:

- Tool registration and lifecycle APIs, including child instances and shared state references.
- Editing API and undo implementation under the settled last-write-wins rule.
- Agent integration, the outer application/runtime protocol details and window/workspace composition. Second milestone.

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
