# SDK sketch

Design hypothesis, September 9, 2026. A small core and two extensions now test part of this sketch in [the lifecycle prototype](experiments/core-lifecycle/README.md). Use its SDK.md for implemented APIs; the illustrative APIs below are still provisional. [ARCHITECTURE.md](ARCHITECTURE.md) remains the source of truth.

## Example and goal

A composer creates a Tone, changes its frequency and level, connects its mono output to a device output, and opens a second view of the same Tone. An agent can make the same changes by editing its JSON record. Both views and the sound follow the current record.

Tone is an example for testing the SDK. It does not establish a bundled instrument roadmap. Its saved state is frequency in Hz and linear gain. Oscillator phase and filter memory are runtime state and are not saved.

## Associate capabilities through registration

An extension registers a stable tool name and attaches only the capabilities it needs. A typed tool handle connects those registrations. A view or processor does not need to implement persistence, actions, ownership and every other capability itself.

Illustrative Rust-shaped API, not compilable code:

```rust
#[derive(Serialize, Deserialize, Clone)]
struct ToneState {
    frequency_hz: f32,
    gain: f32,
}

let tone = registry.tool::<ToneState>("example.tone", initial_tone);
let frequency = registry.parameter(tone, "frequency", frequency_binding);
let level = registry.parameter(tone, "level", gain_binding);
let output = registry.output::<MonoAudio>(tone, "audio");
registry.behaviour(tone, ToneBehaviour::new(frequency, level, output));
registry.view(tone, "editor", ToneEditor::new);
```

The state type supplies serialization; it is not redefined in a separate schema. A parameter binding reads and changes a field in that state and declares its default, unit, range and control mapping. It does not store a second base value. Tool creation must use defaults consistent with those declarations.

Parameter setters supply the ordinary typed edits here. Register a named action only when the tool has a meaningful operation beyond setting state. A stateless tool omits saved state. A view-only tool omits behaviour and ports. A headless tool omits views. Exact builder and trait syntax remains open.

Use typed instance and port handles inside Rust. Persist stable instance IDs, tool names and port names. Resolve and check these when loading; Rust types alone cannot validate external JSON or saved connections. Ordinary Rust dependencies share a contract such as `MonoAudio` without sharing the producing tool's implementation.

## One path for state edits

The project service owns the current typed record. Views hold an instance handle and read that record; their local state is limited to interface concerns such as focus and an active gesture.

1. A frequency gesture begins a named edit with the record's before-state.
2. Each update changes the typed state and publishes it. Behaviour receives the new state and both views are notified. Sound can change during the gesture.
3. Finishing writes the record atomically and creates one undo entry. Cancellation applies the before-state through the same path.
4. An agent replaces the JSON record. The watcher decodes it as `ToneState` and submits a typed whole-record replacement to that same editing service. This is one undo entry. The agent does not call a separate editing API.
5. Undo and redo apply recorded state through the same behaviour and notification path. The runtime recognizes its own file writes and does not replay them.

The extension's state application hook accepts the previous state, if any, and the next state. Loading calls it with no previous state. It must handle any valid replacement, not only changes made by its own controls. Decoding and validation happen before publishing; an invalid outside record reports an error and leaves the live state unchanged. The outside file remains available for correction.

Decided: last write wins, always. File edits apply immediately and drags stay active. Later drag updates, undo, redo or cancellation apply as later writes and may overwrite intervening changes. No merging, conflict handling or special synchronization rules.

## Get parameters and ports to audio

The Tone behaviour creates an oscillator and filter in the core's graph and maps the declared output to the filter's mono output. The core owns graph execution and the device endpoint. The interface never calls DSP code directly.

State application runs outside the audio callback. For this fixed graph, it converts the current frequency and gain into a small parameter update. The engine hands that update to the existing processor at a block boundary. The processor keeps its phase and filter memory across parameter edits. Whole-record replacement uses this same path.

The audio callback processes prepared data without file I/O, GPUI work, allocation or locks. Queue representation and handling of rapid updates still need a prototype. The sketch does not promise sample-accurate UI edits; scheduled events use the engine's separate timing contract.

A connection joins typed endpoint handles. The core checks direction and the declared signal contract, including channel layout and meaning, then connects their graph endpoints. Device output is a core endpoint. Connections are saved once in the project, not copied into Tone's state. Routing edits send a new schedule while surviving processors keep their state, so they do not stop playback (see ENGINEERING.md section 3). Frequency and gain edits must not stop it either.

If a parameter later supports modulation, the saved field remains its base value. A declared modulation input and an explicit combination rule produce the effective audio value. Neither the modulation signal nor the effective value overwrites the record. Tone does not need modulation to test the first lifecycle.

## Trace the lifecycle

| Step | What happens |
| --- | --- |
| Register | Loading the compiled extension registers Tone's state type and its capabilities. Nothing sounds and no instance record is created. |
| Create | The project allocates an instance ID, stores its initial state and invokes behaviour from empty. Opening its editor is a separate workspace operation. |
| Connect | The project stores a connection from Tone's declared audio output to a compatible device endpoint. The engine resolves both endpoints. |
| Edit in a view | The view publishes a typed edit through the project service. Behaviour updates processor parameters; all views observe the same state. Finishing persists it. |
| Edit a file | The watcher submits a decoded whole-record replacement. The same behaviour updates the already running processor. No compilation occurs. |
| Edit extension code | The outer application builds while the current runtime remains usable. On success it stops playback, flushes finished state and restarts. Failure retains the old executable. |
| Restore | Register types, load instance records and ownership, create behaviour from empty, resolve connections, then restore views. Phase and undo history reset; saved values and routing survive. Playback stays stopped. |
| Close a view | Remove its workspace entry. The Tone instance and its sound remain. |
| Delete | Remove the instance's graph contribution, connections and record folder through the project service. Views close or show that their target was deleted. Other instances survive. |

## Owned children and references

To test composition later, a Two-tone tool can own two Tone instances and expose their selected ports. Ownership is the folder tree: a child's folder sits inside its parent's (see Project storage in ARCHITECTURE.md). Child state stays in the child records. The parent never embeds another copy of their frequency and gain.

Creating a fresh composite creates its children once. Restoring it resolves those existing children; its state application hook must not create duplicates. Deleting it deletes its owned children and their connections. Ownership must have one parent per child and no cycles.

A second editor or an existing mixer can reference either Tone without owning it. A reference does not keep an instance alive or cause cascade deletion. Core-owned connections are removed when an endpoint is deleted. Extension-held references resolve to an optional instance so the extension can display a missing target without deleting unrelated content. Closing either editor leaves the Tone intact.

The distinction should be visible in SDK operations such as `create_child(parent, ...)` and `resolve(id)`. Do not infer ownership from a reference, a port connection or a view hierarchy.

## Smallest next prototype

The approved prototype starts with one registered Tone, two independent instances, and two GPUI views of one instance. Include the mono output, live frequency and gain edits, typed record replacement, one-step undo, persistence, deletion and successful/failed code reload. Keep it in an isolated experiment.

Verify that both views and rendered samples follow interface and file edits without compiling, undo restores the prior record, reopening restores values and connections, closing a view preserves content, and deletion removes its sound and routing. Measure one DSP source edit through rebuild and readiness. Use offline rendering first to check the state-to-processor contract; audible device output and callback timing need a separate check before claiming live audio works.

Owned children can be the next addition once this lifecycle works. Scheduling, modulation, agent integration and a general workspace editor remain outside this first prototype. An independent agent authoring a view from the resulting SDK notes is a later authoring check, not something this sketch has verified.
