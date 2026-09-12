# Extension authoring for the lifecycle prototype

This SDK is a working experiment. Read this file and `tone/src/` to build another extension. Rust APIs come from the implementation; the earlier SDK sketch is not an API reference.

## What the core provides

`sound-core` owns typed records, JSON serialization, edits, gesture grouping, undo/redo, record polling, saved mono output connections and offline processor execution. It has no GPUI dependency. `sound-ui` connects the same project to GPUI through an observable `Entity<Session>` and optional view registration.

An extension supplies a state type, validation, optional processor and optional view. It does not own another state store, write project files from its view, or implement undo.

## Register state and behaviour

Derive `Clone`, `Serialize` and `Deserialize` for saved state. Public fields make typed interface edits straightforward. Ordinary Rust defaults create initial values; there is no parameter metadata layer yet.

```rust
let tool = registry.register("my-extension.my-tool", validate);
registry.processor(tool, "audio", |state| Box::new(MyProcessor::new(state)));
```

`validate` is a `fn(&MyState) -> sound_core::Result<()>`. Registration returns `Tool<MyState>`, a copyable typed registration handle. Use a stable tool name. Omit `processor` for state without audio behaviour.

Implement `Processor<MyState>`:

- `apply(&mut self, state: &MyState)` receives each valid state replacement on the control path. Update parameters while preserving runtime phase or buffers.
- `render(&mut self, output: &mut [f32], sample_rate: f32)` fills the mono output slice. Do not allocate, lock, access files or use GPUI here. Keep phases and buffers in the processor, not saved state.

The factory receives initial state when the project creates or restores an instance. Invalid state never reaches the processor. All state changes use the same replacement path, including interface edits, file edits and undo.

This experiment executes offline on one thread. It has no device callback or parameter queue. Processor conventions are suitable for testing that future boundary but do not prove realtime safety. Sample rate is supplied by the caller. The host uses 48 kHz.

## Instances and connections

```rust
let instance = project.create(tool, "my-instance", MyState::default())?;
project.connect(&instance, "audio")?;
let restored = project.resolve(tool, "my-instance")?;
let current = project.state(&restored)?;
```

`Instance<MyState>` is a cloneable typed ID, not an owned copy of state. `resolve` checks the tool and Rust type. A connection routes the declared mono audio output into the core's offline mono bus. Wrong port names or tools without processors fail. This prototype supports one declared audio output per tool, not a general graph. Multiple connected instances mix into that bus.

`project.render(&mut samples, sample_rate)` renders only while `project.set_playing(true)`. Opening a project starts stopped. This small rendering switch is not the final transport implementation.

`project.delete(&instance)` removes that instance, its routing and record. It stops rendering and discards undo entries targeting the deleted instance. Deletion undo and owned child instances are not implemented yet. References from open views then resolve as missing. Closing a view does not delete an instance.

## Write a GPUI view

Use `ToneEditor` as the current API example. Its ad hoc styling and controls are not a design system. The agreed [UI guidance](../../ARCHITECTURE.md#ui-design-system) calls for shared styling and built-in controls as the default for agent-authored interfaces, with direct GPUI available for custom musical interactions. Those shared components are not implemented yet. A view stores `Entity<Session>`, an `Instance<MyState>`, and a GPUI subscription:

```rust
let subscription = cx.observe(&session, |_, _, cx| cx.notify());
```

Keep that subscription in the view. Read current state in `render`:

```rust
let state = self.session.read(cx).project().state(&self.instance).cloned();
```

Handle a missing instance by showing it was deleted. Publish edits through the bridge:

```rust
self.session.update(cx, |session, cx| session.change(cx, |project| {
    project.edit(&self.instance, "Adjust tool", |state| {
        state.gain = new_gain;
    })
}));
```

`Session::change` reports an error in host status and notifies observers. Every view of that instance reads the new state. The callback receives the current typed state. Do not cache a saved-state copy in your editor.

Register the view separately from state/behaviour:

```rust
views.register(tool, MyEditor::new);
```

The constructor takes `(Entity<Session>, Instance<MyState>, &mut Context<MyEditor>)` and returns `MyEditor`. `Views::open` builds the editor by tool name and instance ID. Core registration still works without registering or opening any view.

## Gestures, files and undo

For a gesture, use `begin(&instance, label)` to get `Edit<MyState>`, `publish(&edit, |state| ...)` for updates, then `finish(edit)` or `cancel(edit)`. Publish changes behaviour immediately; finish writes the record and adds one undo entry. Cancellation writes the original record. These operations can also be wrapped in `Session::change`.

Agents edit `state/<id>.json` directly, for example:

```json
{"tool":"example.tone","state":{"frequency_hz":330.0,"gain":0.3}}
```

The host polls known records every 50 ms. The core decodes changed bytes, validates the typed state and applies one replacement and undo entry. Own writes are remembered and do not replay. Invalid files leave live state unchanged and remain on disk for correction. External creation/deletion, index changes and type changes require restart in this prototype.

Last write wins for all operations. A file edit leaves a drag active. Later drag updates, undo, redo or cancellation may overwrite intervening edits. No merging or conflict handling.

## Add an extension crate

Add a workspace member with dependencies on `sound-core`, `serde`, and optionally `sound-ui` and `gpui`, all using `.workspace = true`. Expose `register(&mut Registry) -> Tool<MyState>` and, for a GUI, `register_view(&mut Views, Tool<MyState>)` like Tone.

The host calls those functions before `Project::open`, then can seed instances only when creating a new project. Its workspace opens registered views by tool name; it does not need your state or editor implementation. Add your crate to the host's dependencies and register it in `main`.

## Headless project operations

```rust
let mut project = Project::open(&folder, registry)?;
let applied_count: usize = project.poll_files()?;
project.undo()?;
project.redo()?;
let undo_steps: usize = project.undo_len();
let connections: &[sound_core::Connection] = project.connections();
```

`Connection` exposes `instance: String` and `port: String`. `Project::open` takes ownership of the registry; register extension types before calling it. Reopening uses a fresh registry populated the same way. `project.root()` gives the project folder path.

## Verify

Use an isolated temporary folder. Create two independent instances and connect one. Render samples, edit its state, and verify the samples change while the other instance's record stays unchanged. Replace its file, poll, undo and reopen. Reopening must restore values and routing, with rendering stopped and no undo history. Check both views share state in the host.

```sh
CARGO_TARGET_DIR=/private/tmp/sound-tools-timing/target cargo check --offline -p YOUR_CRATE
CARGO_TARGET_DIR=/private/tmp/sound-tools-timing/target cargo test --offline -p YOUR_CRATE
```

The target directory above reuses this machine's cached GPUI build. Another machine can omit it. GPUI is pinned to 0.2.2 with runtime shaders; no new dependency is needed for this exercise.
