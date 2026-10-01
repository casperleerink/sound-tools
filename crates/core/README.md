# sound-core

The engine, the musical clock, the transport and the live project folder. Every extension is
built on this crate. Extensions will more and more be written by an agent, so this guide gives
the mental model and the rules that the types cannot enforce. The code documents each call.

## The two halves of an extension

- A **processor** makes sound. It runs on the audio thread, one block at a time
  (`Processor`: `ports`, `prepare`, `update`, `process`).
- A **tool** is what a composer creates and a project saves. It has a record, a JSON file under
  `state/`, whose `state` is your type (`State`). It has a **behaviour**: one function that turns
  that state into processors, updates and connections.

The two halves never share memory. The control side sends a processor an `Update`. It
applies at the start of the next block. An extension never touches threads, rings or the
engine itself.

## How state reaches the engine

1. Something changes a record: a view, an agent writing the file, an undo, or loading the
   project. All of these take the same path.
2. The record is checked by `State::validate`. An invalid record does not load. The instance
   keeps its last valid state, and the problem is listed in `problems.txt` by path and field.
3. The behaviour runs with the new state. It declares everything the instance needs, every
   time: `context.processor(name, create)`, `context.update(node, update)`,
   `context.connect(..)`, `context.output(name, endpoint)`.
4. The core compares with the last run and sends only the difference. A processor declared
   again is the same processor, with its phase and voices. One you stop declaring is removed.
   You never remove anything yourself.
5. All behaviours of one edit group land in one engine batch, in the same block. If one fails,
   the whole group is rejected and nothing changes.

An owner reads its children with `context.children::<S>()` and finds their ports with
`context.child_input(name, port)` and `context.child_output(name, port)`. Children run before
their owner. The folder tree is the ownership tree.

## How edits work

Every change is an edit on the `Project`: `begin`, any number of `publish`, then `finish` or
`cancel`. Or `commit` for one step. A finished edit is one undo step, and the core writes the
reverse for you. The file is written once, when the edit finishes. Last write wins: a file edit
by an agent during a drag applies at once, and the next move of the drag writes over it.

## Rules that are easy to miss

- **The behaviour must accept any valid state.** Agents write files by hand, so it gets states
  your own interface never makes.
- **Nothing on the audio thread may allocate, lock, do I/O, log or free heap memory.** Send
  snapshots as an `Arc` and swap them in `update`. The engine carries the old one back to the
  control thread, where it is dropped. The realtime sanitizer checks this in CI
  (`RTSAN_ENABLE=1`).
- **Extensions never depend on each other.** Two extensions that must agree share a small
  contract crate, as `crates/notes` holds the note event and the port names. An owner finds a
  child by port name, not by tool, so any tool with those ports fits.
- **Save musical time in ticks**, never seconds or frames. Keep runtime state such as phase and
  voices in the processor, never in the record.
- **Name the field in every validation error.** An agent fixes its edit from that message. Keep
  `#[serde(deny_unknown_fields)]`, so a misspelled field is an error and not silence.
- **The form of a record is fixed for good.** `OWNS_CHILDREN` decides between `<name>.json`
  and `<name>/instance.json`. Do not change it later: old records would be in the wrong place.
- **Use `context.problem` for a state you can play only in part**, such as a missing file or
  plugin. A `BehaviourError` is for a real fault, and it rejects the whole group.
- **Keep no engine handle outside the behaviour context.** The context rolls back with a
  rejected group. A static or a captured handle would not.
- **Nothing smooths for you.** A parameter that jumps clicks. Use `Smoothed`.
- **A timeline processor emits what starts in `transport.tick_range`**, at
  `transport.offset_of(tick)`. It never converts time itself. It must release what it holds when
  `transport.jumped` or `transport.stopped_playing` is set, or a paused project sounds forever.
- **Latency is declared, not handled.** A processor whose output lags says so with
  `Processor::latency`, and changes it only in `update`. The engine leads everything before it.
- **There are no schema versions or migrations.** A new field needs a default so old records
  still load.
- **An asset is opaque bytes, not state.** Plugin state and recorded takes are assets
  (`context.assets()`). They are not undoable. `Assets::create` never writes over a file.
- **A derive is for a record whose meaning is a rewrite of other state**, such as a tempo fit
  that rewrites the tempo map. It runs inside the same edit group, so it is one undo step. It
  does not run on load or undo, because the files already hold its result.

## What an agent reads

An extension registers an agent doc (`registry.agent_doc`). The runtime writes it into every
project as `agent-docs/<name>.md`. A test loads every example record in it, so a doc whose
example does not load fails the build. The doc is the one place that explains the record
format. Keep each doc about one task, and name the task in its `when` line.

## Copy an example

| You want | Start from |
| --- | --- |
| The smallest tool: one record, one processor | `extensions/tone/src/lib.rs` |
| An effect with parameters, a card and an agent doc | `extensions/filter` |
| An instrument that plays notes | `extensions/instrument` |
| An owner with many small child records | `crates/core/tests/project/tools.rs` |
| A processor driven by the timeline, with snapshots | `extensions/arrangement/src/sequencer.rs` |
| A device whose numbers an automation lane can move | `extensions/filter` (`AutomationInput`, `Automated`) |
| A derive | `extensions/fit-tempo/src/lib.rs` |
| A processor that must be released on the audio thread | `extensions/plugin-host/src/processor.rs` |
| A test of a tool on a real project folder | `extensions/tone/tests/tone/project.rs` |
