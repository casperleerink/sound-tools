# Technical architecture

This document records architecture decisions and proposals. The product goals are in [CONCEPT.md](CONCEPT.md). No implementation has been validated yet.

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

The proposed SDK provides shared components and recommended patterns, with direct GPUI access for custom interfaces. The exact set of helpers remains open. Audio execution and saved musical data remain independent of GPUI.

Use one window with an agent sidebar and the project's musical application in the main area. The application supplies this basic layout; extensions define the musical workspace and can provide transport controls. A fixed top bar is not required.

## Outer application and project runtime

The outer application and project runtime run as separate processes. The outer application manages the project folder, agent conversation, extension source code and builds. It starts and stops the runtime and remains available while the runtime rebuilds or restarts.

Support one open project at a time for now, with one active project runtime and one audio engine. Simultaneous project sessions are not required.

The project runtime runs the Rust core and enabled extensions. It owns live musical state, the audio engine and the tools' custom interfaces. It can also run without graphical interfaces for project inspection, editing and audio rendering by agents.

A defined API connects the two processes. The outer application can request project edits, transport operations and saves; the runtime reports state and errors. API edits use the same typed actions as musical interfaces. External agents can use this access without opening the graphical application.

The runtime is the single owner of live musical state. The outer application requests a save before reload, and the replacement runtime restores the saved project. Agent conversation state survives the reload in the outer application.

The provisional rendering approach lets the runtime draw the entire window, including the agent sidebar. The outer application manages the agent session in the background and supplies conversation data to that sidebar. This keeps the visible interface in one process while preserving agent sessions across runtime restarts. The window may briefly close and reopen on reload; validate the presentation and reload mechanics with a prototype. The communication protocol remains open.

Users should be able to sign in to different AI providers and use their supported subscriptions through the integrated agent. Interchangeable coding agents are not a requirement. Building on Pi is an acceptable direction, subject to verifying its integration and the provider subscription flows we need. The project runtime and extension SDK remain independent of the agent implementation. Specific providers and authentication support remain to be decided and verified.

## Builds and reloads

Changes to extension code, enabled extensions or runtime structure may stop playback and use the project reload workflow. Seamless replacement of running code or audio graphs is not required. Recompile when code or build dependencies change; reloading existing compiled functionality does not inherently require compilation. Parameter changes during playback must work without compilation or project reload.

The proposed build structure separates the engine, SDK and application UI, with one crate per extension initially. Reuse cached dependencies across builds.

Compile enabled extensions into the project runtime executable. The composer can keep using the current runtime while the agent edits source and builds. After a successful build, the outer application automatically stops playback, requests preservation of the latest project state, then restarts the runtime and reopens the project with playback stopped. A failed build reports errors and retains the previous working executable. Changes using already compiled functionality need no compilation. Validate build and restart behaviour with a prototype.

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

A project is a folder containing JSON state and separate assets. This layout is illustrative:

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

`project.json` records the project format version, enabled extensions and their versions, saved instance index, and core-owned connections. Stable identifiers link records independently of display names.

Each extension defines its saved data using Rust types. The core normally handles serialization, writing and loading. JSON is the storage format; live extension code works with typed state.

Save separate records for independently persisted instances. An arrangement may contain all its tracks, clips and notes in one record. Individual notes and parameters do not require separate files.

Each record identifies its type and schema version. Extensions can register additional asset files for large or unusual data.

Importing a sample copies it into the project's assets by default. Saved references use that project-owned copy, so moving or deleting the original file does not break the project and its samples travel with it.

Each project keeps its own editable copies of the extensions it uses. Agent changes for one piece do not modify another project's extensions. Project templates supply starting copies; bringing later extension improvements into an existing project is an explicit update. Extension source travels with the project.

Preserve musical meaning, such as frequency ratios and rhythm groupings, in saved state. Keep workspace layout separate from musical content. Closing a view does not remove its instrument.

Missing extensions leave their saved data intact. Saving must protect the last complete save if writing fails. The mechanisms for coordinated file writes and extension version resolution remain open.

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

The core implements saving, undo/redo and notification delivery. The SDK exposes these services; extension authors choose when to call them and how they fit their tool's workflow.

Interfaces and agents use the same typed editing actions. Authors choose meaningful edit boundaries and can group multiple actions into one undo step. The SDK records affected project state and handles restoration, so authors do not need to implement the reverse of each state edit.

Agents and composers can edit project state concurrently through existing actions without rebuilding. Conflicting writes use last-write-wins semantics, ordered by application in the runtime. Stale edits do not require conflict rejection or agent reapproval. The exact write granularity and interaction with grouped undo remain implementation decisions.

For a drag gesture, begin an edit, publish updates during the drag, then finish it as one named undo step. Cancellation restores the original state. Playback and other views can respond to published updates before the gesture finishes.

Publishing a state edit through the SDK automatically notifies affected views. User-facing messages remain an explicit extension choice. Finishing an edit does not itself require saving to disk.

Undo and redo history are session-only and reset when the project closes or the runtime reloads. Preserve current musical content and settings through the normal save/reload workflow; persisting edit history is not required.

Provide recommended patterns and the underlying operations for custom workflows. The exact edit API, undo storage mechanism and grouping of overlapping edits remain open within the last-write-wins policy.

## Next decisions

- Tool registration and lifecycle APIs, including child instances and shared state references.
- Editing API and undo implementation, including overlapping edits.
- Audio graph execution, scheduling and transport notification APIs.
- Agent integration, the outer application/runtime protocol and window/workspace composition.

## Verification

- An external agent builds a custom GPUI editor from SDK docs and examples, and embeds it twice with independent state.
- A composite tool reuses child tools, exposes selected ports and restores their state without duplication. An alternative view edits the same musical content.
- Editing one extension reuses unchanged build dependencies; measure build and restart time.
- Reload restores musical content and settings. Compilation failure leaves the previous executable usable.
- Reloading the project runtime preserves the outer application's agent conversation. An external agent can inspect, edit and render a project without opening graphical interfaces, using the same editing actions as the UI.
- Parameters change during playback without rebuilding.
- A drag updates playback and shared views, creates one undo step, and can be cancelled. Undo/redo restores grouped project edits made through either an interface or the agent.
- Modulation changes the effective parameter value while preserving its saved base value; reopening restores modulation connections and settings.
- Compatible event and signal ports connect, incompatible contracts produce useful errors, and scheduled events reach processors at the intended sample positions.
- Independent musical clocks schedule precisely on the shared engine. Core transport controls apply consistently, and open-ended projects do not require preparing their entire duration.
- Pause holds position, play resumes, stop returns to zero, and seeking preserves whether playback is running. Seeking clears obsolete scheduled events without replaying skipped events and notifies tools of the position change.
- Saving and reopening restores instances, connections, assets and extension-defined musical data. Missing extensions and failed saves preserve existing work.
