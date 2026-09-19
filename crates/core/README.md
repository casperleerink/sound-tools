# sound-core

The Sound Tools core: the realtime audio engine, the musical clock, the transport and the live project folder. The design and its reasons are in [ENGINEERING.md](../../ENGINEERING.md) section 3 and [ARCHITECTURE.md](../../ARCHITECTURE.md) "Project storage" and "Editing and system services". This file is the API guide for extension authors and later build steps.

An extension has two parts. Processors make sound on the audio thread: see "Write a processor". Tools give processors saved state, a place in the project and live edits: see "Write a tool". Read both. `extensions/tone/src/lib.rs` is the smallest complete example, and `tests/project/tools.rs` is a parent tool with children.

## The two halves

`Engine::new(config)` returns an `EngineControl` and an `Engine`.

- `Engine` is the audio side. Give it to `OutputDevice::start`, or call `engine.process_block(&mut interleaved_samples)` yourself to render offline or in tests. Same code path either way.
- `EngineControl` is the control side. It stays on a normal thread. It owns the graph, sends edits and drops everything the audio thread hands back.

Call `control.poll()` regularly (a UI frame or a few milliseconds). It retries edits that did not fit in the ring, drops returned processors, schedules and snapshots, and returns the latest `EngineStatus`: counters, whether the project plays and the playhead. Once the `Engine` is gone, for example because its stream stopped, `poll` returns `Err(EngineStopped)` and waiting edits are dropped instead of piling up.

Extensions never touch threads, rings or the engine value. They write processors.

## Write a processor

```rust
use sound_core::{AudioOutput, Ports, PrepareConfig, ProcessContext, Processor};

pub struct Tone { /* parameters, phase */ }

impl Tone {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Tone {
    type Update = ToneParameters;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }
    fn prepare(&mut self, config: &PrepareConfig) { /* control thread, may allocate */ }
    fn update(&mut self, update: &mut ToneParameters) { /* audio thread, block start */ }
    fn process(&mut self, context: &mut ProcessContext<'_>) { /* audio thread */ }
}
```

See `extensions/tone/src/lib.rs` for the full example.

Rules for `update` and `process`: no allocation, locks, I/O, logging or drops of heap values. The realtime sanitizer checks this in CI.

### Ports

Declare each port as a constant handle: `AudioInput::new(0)`, `AudioOutput::new(0)`, `EventInput::<MyEvent>::new(0)`, `EventOutput::<MyEvent>::new(0)`. Indices count from 0 per kind. Build `Ports` from the same constants in index order. `process` uses the same constants, so the declared event type and the type you read cannot disagree.

In `process`:

- `context.audio_inputs.get(port)` gives `&[f32]`. Unconnected inputs are silent. Several connections to one input arrive summed.
- `context.audio_outputs.get(port)` gives `&mut [f32]`. Outputs start silent.
- `let [left, right] = context.audio_outputs.get_many([Self::LEFT, Self::RIGHT]);` gives several outputs at once, to write them in one loop.
- `context.event_inputs.get(port)` gives `&[Timed<E>]`, sorted by `offset`, the frame offset within this block. Several connections arrive merged.
- `context.event_outputs.push(port, offset, event)` sends an event.
- `context.frames` is 1 to `MAX_BLOCK` (64). `context.start_frame` is the engine time of the block's first frame.

A handle that matches no declared port (wrong index, wrong event type, or the same output twice in `get_many`) never panics on the audio thread. Reads are empty, writes go nowhere, and each use counts in `EngineStatus::port_misuses`. Anything above zero there is a bug in a processor.

### Schedule from the transport

Engine time (`context.start_frame`) always runs, so live instruments and tails keep sounding. The project position is separate and moves only while the project plays. `context.transport` describes this block on the project timeline:

- `playing`: whether the project plays.
- `stopped_playing`: true for the one block where the project stopped playing, by a pause or a stop.
- `tick_range`: the ticks that land on a frame of this block, as a half-open range. While playing, each block starts at the tick where the previous block ended. This holds for every device buffer size, for tempo changes inside a block and for a tempo map change. So every tick belongs to exactly one block. While not playing the range is empty.
- `offset_of(tick)`: the frame offset inside this block where a tick of `tick_range` lands. Pass it to `event_outputs.push`.
- `jumped`: true for one block after a seek or a stop. Nothing between the old and the new position is replayed. Decide what that means for your tool, for example release held notes.
- `frame_range`: the same block in project frames. A tempo map change moves the frame position, because it keeps the tick position. Schedule by ticks unless your data is in frames.
- `clock`: the `Clock`, for anything else, for example `tempo_at(tick)` or the time signature.

A timeline-driven processor emits what starts inside `tick_range`. It never converts or rounds time itself:

```rust
fn process(&mut self, context: &mut ProcessContext<'_>) {
    let transport = &context.transport;
    if transport.jumped || transport.stopped_playing {
        // Send a note off for every held note. After a jump the notes at the new position
        // come through `tick_range`. After a pause nothing comes until the project plays.
    }
    for note in self.snapshot.notes_starting_in(&transport.tick_range) {
        if let Some(offset) = transport.offset_of(note.start) {
            context.event_outputs.push(Self::NOTES, offset, note.on());
        }
    }
}
```

Starting notes needs no `if transport.playing`: the range is empty while the project does not play. Each note is sent exactly once, on the frame the clock gives for its tick. Ending notes is different. A note off that lies after the pause position is never reached, so a processor must release its held notes when `stopped_playing` is set, and also when `jumped` is set. Without this a paused project sounds forever. The `Beats` processor in `tests/transport.rs` is a small complete example.

### Events

Any `Copy + Send + 'static` type is an event. Two extensions share an event type through a small contract crate. The core does not know the type. Each event port holds `EngineConfig::event_capacity` events per block. More are dropped and counted in `EngineStatus::event_overflows`.

### Updates: parameters, snapshots, anything from the control side

`Processor::Update` is the one message type a processor receives from the control side. It applies at the start of the next block.

- Parameters: make `Update` a small `Copy` struct and copy it in `update`.
- Immutable data snapshots: make `Update` (or a variant of it) an `Arc<Snapshot>` and `std::mem::swap` it with the one the processor holds. The engine carries the old one back and the control thread drops it. Never drop an `Arc`, `Box` or `Vec` in `update`.
- An event from the UI that should happen now, such as a preview note: make it a variant of `Update`.

## Write a tool

A tool is what composers create and projects save. An instance is one use of a tool. It has one JSON record in the project folder, and it may own child instances.

### The project folder

```text
my-piece/
  project.json                format, enabled extensions, tempo map, connections
  state/
    tone-a.json               the instance `tone-a`: no children, so one file
    bank/                     the instance `bank`: it has children, so a folder
      instance.json           its own record
      output.json             the child `bank/output`
      level-1.json            the child `bank/level-1`
```

- The id of an instance is its path under `state/` without `.json`: `bank/level-1`. It never changes. Put display names in the state.
- Names use lowercase letters, digits, `-` and `_`. `instance` is reserved. Every folder is an instance and needs its `instance.json`.
- The folder tree is the ownership tree. Deleting a folder deletes the instance and everything it owns. Moving a file to another folder gives it another owner.
- A record is `{"tool": "<name>", "state": {...}}`. The state is your type.

Agents edit these files while the runtime runs. Your tool gets those edits through the same path as interface edits, so you write no file code.

### State

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToneState {
    pub frequency_hz: f32,
    pub gain: f32,
}

impl State for ToneState {
    const TOOL: &'static str = "tone";

    fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.gain) {
            return Err(format!("gain must be from 0 to 1, not {}", self.gain));
        }
        Ok(())
    }
}
```

- The state type is the handle of the tool. Every typed call names it: `project.resolve::<ToneState>(&id)`, `context.children::<ClipState>()`.
- `TOOL` is the name in records. Use `extension.tool` when an extension has several tools, for example `arrangement.clip`.
- `validate` runs for files and for interface edits. Name the field in the message: an agent fixes its edit from it. Prefer types that cannot hold a wrong value, such as `Ticks`. Keep `deny_unknown_fields`, so a misspelled field is an error.
- Save musical meaning in ticks, not seconds. Keep runtime state such as phase and voices in the processor, never in the state.
- There are no schema versions and no migrations. An instance id in a state (`InstanceId` derives serde) is a reference: it owns nothing. Resolve it with `project.resolve`.

### Register

```rust
pub const EXTENSION: &str = "tone";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<ToneState>(EXTENSION)?.behaviour(apply);
    Ok(())
}
```

The runtime calls `register` of every bundled extension before it opens the project. A project loads the records of a tool only when `project.json` lists its extension under `extensions`. A new project enables every registered extension. Records of other tools stay on disk untouched and show up in `project.problems()`.

A tool without `.behaviour(...)` is plain data. Its owner reads it. Clips are like this.

### Behaviour: from state to the engine

A behaviour is one function. It runs for every valid state of an instance, whatever the source: loading, an interface edit, a file edit, undo. Loading is the same call on an instance with no processors yet. It must accept any valid state, not only steps its own interface makes.

```rust
fn apply(state: &ToneState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let tone = context.processor("oscillator", || Tone::new(*state))?;
    context.update(tone, *state)?;
    context.output("audio", OutputEndpoint::new(tone, Tone::OUTPUT));
    Ok(())
}
```

Declare everything the instance needs, every time. The core compares with the last run and sends only the difference:

- `context.processor(name, create)` gives the processor the instance keeps under `name`. `create` runs only the first time. After that you get the same processor back, with its phase, voices and buffers. A name you stop declaring is removed from the engine. So is everything when the instance is deleted. You never remove anything yourself.
- `context.update(node, update)` sends `Processor::Update`: parameters, or an `Arc` snapshot. Send the current values on every run. It costs no compile.
- `context.connect(connection)` makes a connection of your own: between your processors, to a child's port, or to the device with `output.to_device(channel)` for `channel in 0..context.device_channels()`. A connection you stop declaring is disconnected. These are not saved in `project.json`.
- `context.output(name, endpoint)` and `context.input(name, endpoint)` name a port. Named ports are what `project.json` connections and your owner can use. `OutputEndpoint::new(node, Processor::PORT)` makes one. You may pass up the endpoint of a child.

All behaviours of one edit group run inside one engine edit: one batch, at most one compile, everything lands in the same block. If any behaviour returns an error, or the graph refuses (type mismatch, cycle), the whole group is rejected and nothing changes. Return an error only for real faults. For a state you can play partly, such as a missing child, play what you can.

Do not keep engine handles in statics or captured variables. The context holds them, and it rolls back with a rejected group.

### Children

An owner reads its children and their ports:

- `context.children::<ClipState>()` gives `(name, &state)` of every owned child that holds a `ClipState`, in name order. Direct children only.
- `context.child::<SynthState>("instrument")` gives the state of one child by name.
- `context.child_output("instrument", "audio")` and `context.child_input("instrument", "notes")` give a port that the child's behaviour named. They are `None` when the child is missing or has no such port. Match on the port name, not on the child's tool, so any tool with that port fits.

A behaviour runs again when its own record changes and when anything below it is created, changed or deleted. Children run before their owner, so the ports are there. This is the whole pattern for a parent with many small records and one processor. From `tests/project/tools.rs`:

```rust
fn apply_bank(state: &Bank, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    // One immutable snapshot of all child records, swapped into the one processor.
    let levels: Vec<f32> = context.children::<Level>().map(|(_, level)| level.value).collect();
    let summer = context.processor("summer", Summer::default)?;
    context.update(summer, BankUpdate { gain: state.gain, levels: Arc::new(levels) })?;

    // Through the owned `output` child when it exists, then to the device. No project.json edit.
    let mut output = OutputEndpoint::new(summer, Summer::OUTPUT);
    if let Some(input) = context.child_input("output", "in")
        && let Some(child_output) = context.child_output("output", "out")
    {
        context.connect(output.to(input))?;
        output = child_output;
    }
    context.connect(output.to_device(0))?;
    context.output("out", output);
    Ok(())
}
```

For the arrangement this reads: a track owns clip records and one `instrument` child. Its behaviour builds one snapshot from `children::<Clip>()`, sends it to its one sequencer processor, connects the sequencer's event output to `child_input("instrument", "notes")` and the child's `audio` output to the device. An agent adds a part by writing one clip file, and a whole track by writing one folder. Both arrive as one group: one snapshot, one batch. In `Processor::update`, swap the `Arc` with `std::mem::swap` and never drop it there.

Rebuilding a snapshot of a hundred small records on every change is cheap. If a tool needs to skip work, it can compare inside its behaviour. The core passes no previous state.

### Read and edit from an interface

Views hold an `Instance<S>` and read the current state when they render. They keep no copy.

```rust
let tone: Instance<ToneState> = project.resolve(&id)?;      // None: gone, or another tool
let state: Option<&ToneState> = project.state(&tone);       // None once it is deleted
for (clip, state) in project.children::<ClipState>(track.id()) { /* ... */ }
```

Every change is an edit: `begin`, any number of `publish`, then `finish` or `cancel`. `begin` returns a `ProjectEdit`. It is a plain value that a view keeps for the length of a gesture.

```rust
// A drag. Sound and views follow each publish. No file is written until the end.
let mut edit = project.begin("Change frequency");
project.update(&mut edit, &tone, |state| state.frequency_hz = 330.0)?;   // per mouse move
project.finish(edit)?;      // writes the record once, one undo step
// or: project.cancel(edit)?;   applies the state from before the drag

// Several changes as one group and one undo step.
let mut changes = Changes::new();
let track = changes.create(arrangement.child("bass")?, TrackState::default());
changes.create(track.id().child("instrument")?, SynthState::default());
changes.delete(old_clip.id());
changes.connect(SavedConnection::to_device(PortReference::new(tone.id(), "audio"), 0));
changes.set_tempo_map(tempo_map);
project.commit("Add track", changes)?;      // begin + publish + finish

project.undo()?;    // Some(label), or None when there is nothing to undo
project.redo()?;
```

- `publish(&mut edit, changes)` applies a whole `Changes` group at once: one engine batch. `update` is the short form for one record. `changes.set` replaces a whole state. `create` with an id inside another instance makes an owned child. The owner must exist or come earlier in the same group.
- `delete` takes everything the instance owns and the `project.json` connections that name them. Undo brings all of it back.
- Last write wins everywhere. A file edit during a drag applies at once and the drag goes on. The next publish overwrites it. The undo step of a finished edit runs from the state before its first publish to the state at the finish.
- You never write the reverse of an edit. The core records the records before and after.
- An invalid state is rejected with `ProjectError::InvalidState` and nothing changes.
- A move is a delete and a create in one `Changes` group.

### Follow changes

The project lives on one thread. Whoever owns it calls, regularly:

```rust
project.engine().poll()?;      // the engine: returns the status, frees what came back
project.poll()?;               // the watcher: applies outside changes after 100 ms of quiet
for event in project.drain_events() { /* refresh what is named */ }
```

`ProjectEvent` is `Created(id)`, `Changed(id)`, `Deleted(id)`, `ProjectFileChanged` or `ProblemsChanged`. Events carry no state. A view of a parent that shows its children refreshes when `id.is_inside(parent)`. `project.problems()` lists files that are not live, with the path and the field. Interface edits, file edits, undo and redo all produce the same events.

Transport goes through `project.engine()`: `play`, `pause`, `stop`, `seek`. Change the tempo map with `changes.set_tempo_map`, not on the engine, so it is saved and undoable.

### Test a tool

Open a project on a temporary folder with an offline engine, write files, and call the function the watcher calls. No device, no timing.

```rust
let (control, mut engine) = Engine::new(EngineConfig::new(48_000, 2));
let mut project = Project::open(folder.path(), registry, control)?;
std::fs::write(&path, r#"{"tool": "tone", "state": {"frequency_hz": 330.0, "gain": 0.25}}"#)?;
project.apply_outside_changes(&[path])?;     // one group, one undo step
engine.process_block(&mut interleaved);      // assert on samples
```

`extensions/tone/tests/tone/project.rs` and `tests/project/` show this. `Project::open_read_only` opens without the lock and never writes. The runtime uses it for `--inspect` and `--render`.

## Edit the graph

Tools do not call this API. A behaviour declares processors and connections, and the project makes the engine edit. See "Write a tool". This section is for tests and for code below the project layer.

```rust
let mut edit = control.edit();
let tone = edit.add_processor("state/tone", Tone::new(parameters))?;   // Node<Tone>
edit.connect(Connection::to_device(tone.id(), Tone::OUTPUT, 0))?;
edit.connect(Connection::new(source.id(), Source::NOTES, synth.id(), Synth::NOTES))?;
edit.update(tone, new_parameters)?;
edit.remove_processor(old.id())?;
edit.commit()?;

control.update(tone, new_parameters)?;   // shortcut for an edit with one update
```

- An edit is all or nothing. `commit` validates and compiles the whole graph first. On error nothing is sent and the graph is unchanged. All changes of one edit apply in the same block.
- The name given to `add_processor` must be unique and stable, for example the instance path. Independent processors run in name order, so the same project always compiles to the same schedule.
- Routing changes send a new schedule. Processors that stay keep their state.
- `GraphError` is typed: `Cycle` names the connection that closes the cycle, `PortTypeMismatch` names the connection with different types on its ends, and so on. Feedback connections do not exist yet, so every cycle is an error.
- `Node<P>` types the updates for processor `P`. `NodeId` is the untyped form used in connections.

## Transport

```rust
control.play();                   // advance from the current position
control.pause();                  // hold the position
control.stop();                   // stop and return to zero
control.seek(Ticks(3840));        // move, keep playing or stay stopped
control.set_tempo_map(tempo_map); // keeps the musical position

let status = control.poll()?;
status.playing; status.playhead_tick; status.playhead_frame;
```

Each call is a message to the audio thread. It applies at the start of the next engine block, at most 64 frames later. `poll` gives the state after the last device callback. A new engine is stopped at zero with 120 bpm in 4/4.

`set_tempo_map` compiles the map into a new `Clock` on the control thread and sends it. The audio thread swaps it in, the old clock comes back and is dropped in `poll`. `control.clock()` is the clock set last, for conversions on the control side. A map equal to the current one sends nothing, so loading an unchanged project file does not move the playhead.

`OutputDevice::start` refuses an engine whose sample rate differs from the device, because the clock would play every tempo at the wrong speed. Build the `EngineConfig` from `device.sample_rate()`.

## The musical clock

Musical time is whole ticks, 960 per quarter note (`Ticks`). Project time in audio frames is `Frames`. The two are separate types, so they cannot be mixed up. Save positions and lengths in ticks.

- `Tempo`: beats per minute, a beat being a quarter note. 10 to 1000 bpm, held in steps of 0.001 bpm. `Tempo::from_bpm(93.5)?`.
- `TimeSignature`: numerator 1 to 32, denominator 1, 2, 4, 8, 16 or 32. One per project for now. It converts ticks to and from `BarBeat`, which counts bars and beats from 1 and prints as `bar:beat:tick`, for example `4:3:005`.
- `TempoMap`: the time signature and a list of tempo changes. Steps only, no ramps. The first change is at tick 0 and the ticks go up. This is the saved form.
- `Clock`: a `TempoMap` compiled for one sample rate. `frame_of(tick)`, `tick_at(frame)`, `seconds_of(tick)`, `tick_at_seconds(seconds)`, `tempo_at(tick)`. A lookup is a binary search over the tempo changes.

Invalid values cannot be built: the constructors and the JSON loader return a `ClockError`.

`Clock::frame_of` is the only place where ticks become frames. A tick lands on the frame that contains its exact time, so the exact position rounded down. Each tempo change starts on the frame of its own tick, rounded down the same way, so a tempo change can move the ticks after it early by less than one frame. Every conversion uses the same clock, so all parts still agree. `tick_at(frame)` is the first tick at or after the frame. For every valid tempo and every sample rate from 16000 Hz up (`MIN_EXACT_SAMPLE_RATE`), a tick is at least one frame long, so no two ticks share a frame and `tick_at(frame_of(tick)) == tick`. `OutputDevice` refuses lower sample rates.

The saved JSON, as it will appear in `project.json`:

```json
{
  "time_signature": "4/4",
  "tempo_changes": [
    { "tick": 0, "bpm": 120.0 },
    { "tick": 15360, "bpm": 93.5 }
  ]
}
```

`bpm` is a plain number, `120` and `120.0` both load. `tick` is where the tempo starts.

## Checks

```sh
cargo nextest run -p sound-core -p tone
RTSAN_ENABLE=1 cargo nextest run -p sound-core -p tone   # with the realtime sanitizer
cargo nextest run -p sound-core --run-ignored only ten_thousand --no-capture   # scale numbers
cargo run -p runtime -- my-project                        # runs the folder live on the default device
cargo run -p runtime -- my-project --inspect              # summary, no device, no lock
cargo run -p runtime -- my-project --render out.wav --seconds 4
```
