# sound-core

The Sound Tools core. Today it holds the realtime audio engine, the musical clock and the transport. The design and its reasons are in [ENGINEERING.md](../../ENGINEERING.md) section 3. This file is the API guide for extension authors and later build steps.

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

Starting notes needs no `if transport.playing`: the range is empty while the project does not play. Each note is sent exactly once, on the frame the clock gives for its tick. Ending notes is different. A note off that lies after the pause position is never reached, so a processor must release its held notes when `stopped_playing` is set, and also when `jumped` is set. Without this a paused project sounds forever. The `Click` processor in `crates/runtime/src/main.rs` and the `Beats` processor in `tests/transport.rs` are small complete examples.

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
cargo run -p runtime                                      # plays on the default output device
cargo run -p runtime -- --render out.wav                  # the same scenario offline
```
