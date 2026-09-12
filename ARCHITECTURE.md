# Technical architecture

This document records architecture decisions and proposals. The product goals are in [CONCEPT.md](CONCEPT.md). The small GPUI build-loop experiment below has been validated on macOS. An isolated core lifecycle prototype now implements typed state, persistence, editing, offline processing and GPUI views for Tone and an agent-authored Tremolo. The full application remains unimplemented.

## Terms

| Term | Meaning |
| --- | --- |
| Sound Tools | The application, consisting of the core and installed extensions. |
| Extension | A Rust package that provides tools and supporting functionality, such as shared types and UI components. Projects choose which extensions they use. |
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

Changes to extension code, enabled extensions or runtime structure may stop playback and use the project reload workflow. Seamless replacement of running code or audio graphs is not required. Recompile when code or build dependencies change; reloading existing compiled functionality does not inherently require compilation. Parameter changes during playback must work without compilation or project reload.

The proposed build structure separates the engine, SDK and application UI, with one crate per extension initially. All projects share one Cargo target directory per machine, so the dependency tree compiles once on first launch or after a toolchain change. Opening a new project compiles only its own extension crates and the runtime binary.

Compile enabled extensions into the project runtime executable. The composer can keep using the current runtime while the agent edits source and builds. After a successful build, the outer application automatically stops playback, waits for the runtime to finish writing the project folder, then restarts the runtime and reopens the project with playback stopped. A failed build reports errors and retains the previous working executable. Changes using already compiled functionality need no compilation. Validate build and restart behaviour with a prototype.

### Build-loop experiment, September 9, 2026

A throwaway workspace with one GPUI runtime crate and one statically compiled extension measured the loop on Casper's Mac. A one-line extension edit reached the replacement runtime's first frame in a median 2.2 seconds, with 1.3 seconds of that in the incremental build and link. A failed build kept the old process alive and the executable unchanged. An agent wrote a two-instance custom GPUI view that compiled on its first attempt from public docs.

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

Existing audio plugin hosting, including plugin interfaces, belongs entirely to extensions. A reusable hosting helper is a possible extension idea, not a core feature or a hosting design to specify here.

## Project storage

A project is a folder containing JSON state and separate assets. The project's text files together with its extensions fully describe the work; nothing needed to restore it lives outside them. This layout is illustrative:

```text
my-piece/
  project.json
  state/
    synth-a.json
    synth-b.json
    arrangement-a.json
  assets/
    field-recording.wav
  extensions/
    custom-instrument/
  workspace.json
```

`project.json` records the project format version, enabled extensions by name, saved instance index, and core-owned connections. Extensions have no version numbers: the code in the project's extensions folder is the version, and updating from a newer bundled copy is an explicit copy. Stable identifiers link records independently of display names.

Each extension defines its saved data using Rust types. The core normally handles serialization, writing and loading. JSON is the storage format; live extension code works with typed state.

The project folder is always live. The runtime writes each affected record atomically after an edit finishes, so the files on disk match the running project. The runtime also watches the folder: when a file changes underneath it, it applies the whole record as one typed action and one undo step, through the same path that interfaces use. No field-level diffing; if an interface edit and a file edit touch the same record at the same moment, the last one applied wins for that record. Agent edits and interface edits therefore share one path, including undo and view notifications. The runtime's own writes do not re-apply. There is no separate save step; versions come from git or explicit snapshots.

Extensions must be able to apply new state while running, not only at load. Loading is applying state from empty.

Save separate records for independently persisted instances. An arrangement may contain all its tracks, clips and notes in one record. Individual notes and parameters do not require separate files.

Each record identifies its type. There are no schema versions; keeping code and saved data compatible is the composer's and their agent's responsibility. Extensions can register additional asset files for large or unusual data.

Importing a sample copies it into the project's assets by default. Saved references use that project-owned copy, so moving or deleting the original file does not break the project and its samples travel with it.

Each project keeps its own editable copies of the extensions it uses. Agent changes for one piece do not modify another project's extensions. Project templates supply starting copies; bringing later extension improvements into an existing project is an explicit update. Extension source travels with the project.

Preserve musical meaning, such as frequency ratios and rhythm groupings, in saved state. Keep workspace layout separate from musical content. Closing a view does not remove its instrument.

Missing extensions leave their saved data intact. A failed write must leave the previous file complete. The runtime writes each record to a temporary file and renames it into place, so every file is atomic on its own. An edit that touches several files writes them in dependency order: instance records first, then the index in `project.json`. A crash between renames leaves a valid project with at most one stale record, which normal loading errors surface. Fully atomic multi-file commits are not required.

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

The timeline provides a common time coordinate and scheduling facilities for all tools. It does not require bars, beats, a global tempo, tracks or clips. Timeline views and arrangement workflows remain extension functionality.

Projects can have a finite duration or run without a predetermined end. The runtime must not require preparing the entire duration before playback.

Extensions define musical divisions of time and can maintain independent musical clocks. Their timing must ultimately translate into scheduling on the shared audio engine. The SDK should provide the timing information and conversion facilities needed to do this precisely.

Project position is distinct from the engine's advancing sample count. Audio processing can continue while project playback is paused or stopped, allowing live instruments and effect tails to continue. The exact time representation remains open.

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

[The SDK sketch](SDK_SKETCH.md) proposes a Tone lifecycle and a small next prototype. Its APIs remain provisional. Last-write-wins also applies to active drags, undo, redo and cancellation, as agreed above.

- Tool registration and lifecycle APIs, including child instances and shared state references.
- Editing API and undo implementation under the settled last-write-wins rule.
- Audio graph execution, scheduling and transport notification APIs.
- Agent integration, the outer application/runtime protocol and window/workspace composition.

## Verification

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
