# sound-core

The Sound Tools core. Today it holds the realtime audio engine. The design and its reasons are in [ENGINEERING.md](../../ENGINEERING.md) section 3. This file is the API guide for extension authors and later build steps.

## The two halves

`Engine::new(config)` returns an `EngineControl` and an `Engine`.

- `Engine` is the audio side. Give it to `OutputDevice::start`, or call `engine.process_block(&mut interleaved_samples)` yourself to render offline or in tests. Same code path either way.
- `EngineControl` is the control side. It stays on a normal thread. It owns the graph, sends edits and drops everything the audio thread hands back.

Call `control.poll()` regularly (a UI frame or a few milliseconds). It retries edits that did not fit in the ring, drops returned processors, schedules and snapshots, and returns the latest `EngineStatus` counters. Once the `Engine` is gone, for example because its stream stopped, `poll` returns `Err(EngineStopped)` and waiting edits are dropped instead of piling up.

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

Transport info (playing, position, tempo) will be added to `ProcessContext` with the musical clock.

### Events

Any `Copy + Send + 'static` type is an event. Two extensions share an event type through a small contract crate. The core does not know the type. Each event port holds `EngineConfig::event_capacity` events per block. More are dropped and counted in `EngineStatus::event_overflows`.

### Updates: parameters, snapshots, anything from the control side

`Processor::Update` is the one message type a processor receives from the control side. It applies at the start of the next block.

- Parameters: make `Update` a small `Copy` struct and copy it in `update`.
- Immutable data snapshots: make `Update` (or a variant of it) an `Arc<Snapshot>` and `std::mem::swap` it with the one the processor holds. The engine carries the old one back and the control thread drops it. Never drop an `Arc`, `Box` or `Vec` in `update`.
- An event from the UI that should happen now, such as a preview note: make it a variant of `Update`.

## Edit the graph

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

## Checks

```sh
cargo nextest run -p sound-core -p tone
RTSAN_ENABLE=1 cargo nextest run -p sound-core -p tone   # with the realtime sanitizer
cargo run -p runtime                                      # plays on the default output device
cargo run -p runtime -- --render out.wav                  # the same scenario offline
```
