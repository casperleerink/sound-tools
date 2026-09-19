//! Engine behaviour through the public API only, rendered offline. No audio device needed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use std::cell::Cell;
use std::sync::Arc;

use sound_core::{
    AudioInput, AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventInput,
    EventOutput, GraphError, MAX_BLOCK, Ports, PrepareConfig, ProcessContext, Processor,
};

const OUTPUT: AudioOutput = AudioOutput::new(0);
const INPUT: AudioInput = AudioInput::new(0);

/// Writes one value to every frame. An update replaces the value.
struct Constant(f32);

impl Processor for Constant {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut f32) {
        self.0 = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        context.audio_outputs.get(OUTPUT).fill(self.0);
    }
}

/// Copies its input to its output.
struct Through;

impl Processor for Through {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_input(INPUT).audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let input = context.audio_inputs.get(INPUT);
        context.audio_outputs.get(OUTPUT).copy_from_slice(input);
    }
}

/// Counts the frames it has processed, in its own state. Frame N of the output holds N.
#[derive(Default)]
struct FrameCounter(u32);

impl Processor for FrameCounter {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        assert!(context.frames <= MAX_BLOCK);
        assert_eq!(context.start_frame, u64::from(self.0));
        for sample in context.audio_outputs.get(OUTPUT) {
            *sample = self.0 as f32;
            self.0 += 1;
        }
    }
}

/// An extension-defined event. The core never sees this type by name.
#[derive(Copy, Clone)]
struct Ping {
    level: f32,
}

const PINGS_OUT: EventOutput<Ping> = EventOutput::new(0);
const PINGS_IN: EventInput<Ping> = EventInput::new(0);

/// Sends a `Ping` at each listed engine frame, the way a timeline-driven processor will.
struct Emitter {
    level: f32,
    /// (engine frame, how many pings to send there)
    pings: Vec<(u64, usize)>,
}

impl Processor for Emitter {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(PINGS_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let block = context.start_frame..context.start_frame + context.frames as u64;
        for (frame, count) in &self.pings {
            if block.contains(frame) {
                let offset = (frame - context.start_frame) as usize;
                for _ in 0..*count {
                    let level = self.level;
                    context
                        .event_outputs
                        .push(PINGS_OUT, offset, Ping { level });
                }
            }
        }
    }
}

/// Adds each received ping's level to the output sample at the ping's frame.
struct Receiver;

impl Processor for Receiver {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_input(PINGS_IN).audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let pings = context.event_inputs.get(PINGS_IN);
        assert!(pings.is_sorted_by_key(|ping| ping.offset));
        let output = context.audio_outputs.get(OUTPUT);
        for ping in pings {
            output[ping.offset] += ping.event.level;
        }
    }
}

fn mono() -> (EngineControl, Engine) {
    Engine::new(EngineConfig::new(48_000, 1))
}

/// Renders `frames` frames the way a device with this buffer size would ask for them.
fn render(engine: &mut Engine, frames: usize, device_buffer: usize) -> Vec<f32> {
    let mut output = vec![f32::NAN; frames];
    for buffer in output.chunks_mut(device_buffer) {
        engine.process_block(buffer);
    }
    output
}

fn to_device(edit: &mut sound_core::Edit<'_>, node: sound_core::NodeId) {
    edit.connect(Connection::to_device(node, OUTPUT, 0))
        .unwrap();
}

#[test]
fn odd_device_buffers_skip_no_frames_and_never_exceed_the_block_size() {
    for device_buffer in [1, 63, 64, 65, 480, 513] {
        let (mut control, mut engine) = mono();
        let mut edit = control.edit();
        let counter = edit
            .add_processor("counter", FrameCounter::default())
            .unwrap();
        to_device(&mut edit, counter.id());
        edit.commit().unwrap();

        let output = render(&mut engine, 2_000, device_buffer);
        let expected: Vec<f32> = (0..2_000).map(|frame| frame as f32).collect();
        assert_eq!(output, expected, "device buffer {device_buffer}");
        assert_eq!(control.poll().frames, 2_000);
    }
}

#[test]
fn fan_in_sums_and_fan_out_shares() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(48_000, 2));
    let mut edit = control.edit();
    let one = edit.add_processor("one", Constant(1.0)).unwrap();
    let two = edit.add_processor("two", Constant(2.0)).unwrap();
    let sum = edit.add_processor("sum", Through).unwrap();
    let left = edit.add_processor("left", Through).unwrap();
    let right = edit.add_processor("right", Through).unwrap();
    // Two sources into one input. One output into two inputs.
    edit.connect(Connection::new(one.id(), OUTPUT, sum.id(), INPUT))
        .unwrap();
    edit.connect(Connection::new(two.id(), OUTPUT, sum.id(), INPUT))
        .unwrap();
    edit.connect(Connection::new(sum.id(), OUTPUT, left.id(), INPUT))
        .unwrap();
    edit.connect(Connection::new(sum.id(), OUTPUT, right.id(), INPUT))
        .unwrap();
    edit.connect(Connection::to_device(left.id(), OUTPUT, 0))
        .unwrap();
    edit.connect(Connection::to_device(right.id(), OUTPUT, 1))
        .unwrap();
    // The device output sums too.
    edit.connect(Connection::to_device(one.id(), OUTPUT, 1))
        .unwrap();
    edit.commit().unwrap();

    let mut output = [0.0; 2 * 100];
    engine.process_block(&mut output);
    for frame in output.chunks(2) {
        assert_eq!(frame, [3.0, 4.0]);
    }
}

#[test]
fn a_cycle_is_rejected_by_name_and_leaves_the_engine_unchanged() {
    let (mut control, mut engine) = mono();
    let mut edit = control.edit();
    let source = edit.add_processor("source", Constant(1.0)).unwrap();
    let first = edit.add_processor("first", Through).unwrap();
    let second = edit.add_processor("second", Through).unwrap();
    edit.connect(Connection::new(source.id(), OUTPUT, first.id(), INPUT))
        .unwrap();
    edit.connect(Connection::new(first.id(), OUTPUT, second.id(), INPUT))
        .unwrap();
    to_device(&mut edit, second.id());
    edit.commit().unwrap();

    let closing = Connection::new(second.id(), OUTPUT, first.id(), INPUT);
    let mut edit = control.edit();
    edit.update(source, 5.0).unwrap();
    edit.connect(closing).unwrap();
    let error = edit.commit().unwrap_err();
    let GraphError::Cycle {
        connection,
        description,
    } = &error
    else {
        panic!("expected a cycle error, got {error}");
    };
    let forward = Connection::new(first.id(), OUTPUT, second.id(), INPUT);
    assert!([closing, forward].contains(connection));
    assert!(description.contains("first") && description.contains("second"));
    assert!(error.to_string().contains("closes a cycle"));

    // Nothing of the failed edit was sent, not even its valid update.
    assert_eq!(render(&mut engine, 10, 10), [1.0; 10]);
    assert_eq!(control.poll().batches_applied, 1);
    // The graph still has no cycle, so a later edit compiles.
    control.edit().disconnect(&closing).unwrap_err();
    control.update(source, 2.0).unwrap();
    assert_eq!(render(&mut engine, 10, 10), [2.0; 10]);
}

#[test]
fn connections_are_checked_for_port_type_and_existence() {
    #[derive(Copy, Clone)]
    struct Other;
    struct OtherReceiver;
    impl Processor for OtherReceiver {
        type Update = ();
        fn ports(&self) -> Ports {
            Ports::new().event_input(EventInput::<Other>::new(0))
        }
        fn prepare(&mut self, _: &PrepareConfig) {}
        fn update(&mut self, _: &mut ()) {}
        fn process(&mut self, _: &mut ProcessContext<'_>) {}
    }
    struct Misdeclared;
    impl Processor for Misdeclared {
        type Update = ();
        fn ports(&self) -> Ports {
            Ports::new().audio_output(AudioOutput::new(1))
        }
        fn prepare(&mut self, _: &PrepareConfig) {}
        fn update(&mut self, _: &mut ()) {}
        fn process(&mut self, _: &mut ProcessContext<'_>) {}
    }

    let (mut control, _engine) = mono();
    let mut edit = control.edit();
    let emitter = Emitter {
        level: 1.0,
        pings: Vec::new(),
    };
    let emitter = edit.add_processor("emitter", emitter).unwrap();
    let receiver = edit.add_processor("receiver", Receiver).unwrap();
    let other = edit.add_processor("other", OtherReceiver).unwrap();

    let wrong_event = Connection::new(
        emitter.id(),
        PINGS_OUT,
        other.id(),
        EventInput::<Other>::new(0),
    );
    let error = edit.connect(wrong_event).unwrap_err();
    assert!(
        matches!(error, GraphError::PortTypeMismatch { connection, .. } if connection == wrong_event)
    );
    let audio_to_events = Connection::new(receiver.id(), OUTPUT, receiver.id(), PINGS_IN);
    assert!(matches!(
        edit.connect(audio_to_events).unwrap_err(),
        GraphError::PortTypeMismatch { .. }
    ));
    let events_to_device = Connection::to_device(emitter.id(), PINGS_OUT, 0);
    assert!(matches!(
        edit.connect(events_to_device).unwrap_err(),
        GraphError::PortTypeMismatch { .. }
    ));
    let missing_input = Connection::new(receiver.id(), OUTPUT, receiver.id(), INPUT);
    assert!(matches!(
        edit.connect(missing_input).unwrap_err(),
        GraphError::UnknownInput { .. }
    ));
    let missing_channel = Connection::to_device(receiver.id(), OUTPUT, 1);
    assert_eq!(
        edit.connect(missing_channel).unwrap_err(),
        GraphError::UnknownDeviceChannel(1)
    );
    assert_eq!(
        edit.add_processor("receiver", Receiver).err(),
        Some(GraphError::DuplicateName("receiver".to_string()))
    );
    assert_eq!(
        edit.add_processor("misdeclared", Misdeclared).err(),
        Some(GraphError::PortsOutOfOrder("misdeclared".to_string()))
    );
}

#[test]
fn events_arrive_at_exact_frames_across_sub_blocks() {
    let ping_frames = [0, 1, 62, 63, 64, 65, 127, 128, 479, 480, 512, 513, 1_999];
    for device_buffer in [1, 63, 64, 65, 480, 513] {
        let (mut control, mut engine) = mono();
        let mut edit = control.edit();
        let pings = ping_frames.iter().map(|frame| (*frame, 1)).collect();
        let emitter = edit
            .add_processor("emitter", Emitter { level: 1.0, pings })
            .unwrap();
        // A second source on two of the same frames, to check that merged inputs stay exact.
        let pings = vec![(63, 1), (64, 1), (700, 1)];
        let second = edit
            .add_processor("second", Emitter { level: 10.0, pings })
            .unwrap();
        let receiver = edit.add_processor("receiver", Receiver).unwrap();
        edit.connect(Connection::new(
            emitter.id(),
            PINGS_OUT,
            receiver.id(),
            PINGS_IN,
        ))
        .unwrap();
        edit.connect(Connection::new(
            second.id(),
            PINGS_OUT,
            receiver.id(),
            PINGS_IN,
        ))
        .unwrap();
        to_device(&mut edit, receiver.id());
        edit.commit().unwrap();

        let output = render(&mut engine, 2_000, device_buffer);
        let mut expected = vec![0.0; 2_000];
        for frame in ping_frames {
            expected[frame as usize] += 1.0;
        }
        for frame in [63, 64, 700] {
            expected[frame] += 10.0;
        }
        assert_eq!(output, expected, "device buffer {device_buffer}");
        assert_eq!(control.poll().event_overflows, 0);
    }
}

#[test]
fn event_overflow_is_counted_not_allocated() {
    let mut config = EngineConfig::new(48_000, 1);
    config.event_capacity = 4;
    let (mut control, mut engine) = Engine::new(config);
    let mut edit = control.edit();
    let emitter = Emitter {
        level: 1.0,
        pings: vec![(10, 7), (100, 3)],
    };
    let emitter = edit.add_processor("emitter", emitter).unwrap();
    let receiver = edit.add_processor("receiver", Receiver).unwrap();
    edit.connect(Connection::new(
        emitter.id(),
        PINGS_OUT,
        receiver.id(),
        PINGS_IN,
    ))
    .unwrap();
    to_device(&mut edit, receiver.id());
    edit.commit().unwrap();

    let output = render(&mut engine, 128, 128);
    // Frame 10 is in the first block: 4 of 7 pings fit. Frame 100 is in the second: all 3 fit.
    assert_eq!(output[10], 4.0);
    assert_eq!(output[100], 3.0);
    assert_eq!(control.poll().event_overflows, 3);
}

#[test]
fn one_edit_lands_in_one_block() {
    let (mut control, mut engine) = mono();
    let mut edit = control.edit();
    for (name, value) in [("a", 1.0), ("b", 2.0), ("c", 4.0)] {
        let node = edit.add_processor(name, Constant(value)).unwrap();
        to_device(&mut edit, node.id());
    }
    edit.commit().unwrap();
    assert!(
        render(&mut engine, 200, 1)
            .iter()
            .all(|sample| *sample == 7.0)
    );
}

#[test]
fn a_full_command_ring_loses_no_edit_and_keeps_their_order() {
    /// Accepts only the next number in sequence. Its output is how far it got.
    struct InOrder(u32);
    impl Processor for InOrder {
        type Update = u32;
        fn ports(&self) -> Ports {
            Ports::new().audio_output(OUTPUT)
        }
        fn prepare(&mut self, _: &PrepareConfig) {}
        fn update(&mut self, update: &mut u32) {
            if *update == self.0 + 1 {
                self.0 = *update;
            }
        }
        fn process(&mut self, context: &mut ProcessContext<'_>) {
            context.audio_outputs.get(OUTPUT).fill(self.0 as f32);
        }
    }

    let mut config = EngineConfig::new(48_000, 1);
    config.ring_capacity = 2;
    let (mut control, mut engine) = Engine::new(config);
    let mut edit = control.edit();
    let node = edit.add_processor("in-order", InOrder(0)).unwrap();
    to_device(&mut edit, node.id());
    edit.commit().unwrap();
    for number in 1..=20 {
        control.update(node, number).unwrap();
    }
    assert_eq!(control.pending_edits(), 19);
    assert!(control.command_ring_full() > 0);

    // The return ring is as small as the command ring, so both directions fill up here.
    let mut blocks = 0;
    while control.pending_edits() > 0 {
        render(&mut engine, 1, 1);
        control.poll();
        blocks += 1;
        assert!(blocks < 100, "the pending edits never drained");
    }
    assert_eq!(render(&mut engine, 1, 1), [20.0]);
    assert_eq!(control.poll().batches_applied, 21);
}

#[test]
fn a_full_return_ring_delays_edits_instead_of_dropping_on_the_audio_thread() {
    let mut config = EngineConfig::new(48_000, 1);
    config.ring_capacity = 2;
    let (mut control, mut engine) = Engine::new(config);
    let mut edit = control.edit();
    let node = edit.add_processor("constant", Constant(0.0)).unwrap();
    to_device(&mut edit, node.id());
    edit.commit().unwrap();
    control.update(node, 1.0).unwrap();
    render(&mut engine, 1, 1);
    // Both returned batches stay in the return ring, because nothing polled. The next update
    // fits in the command ring but must wait there.
    control.update(node, 2.0).unwrap();
    assert_eq!(render(&mut engine, 1, 1), [1.0]);
    let status = control.poll();
    assert!(status.return_ring_full > 0);
    assert_eq!(render(&mut engine, 1, 1), [2.0]);
}

#[test]
fn the_slot_table_grows_and_survivors_keep_their_state_across_schedule_swaps() {
    let mut config = EngineConfig::new(48_000, 1);
    config.processor_slots = 1;
    let (mut control, mut engine) = Engine::new(config);
    let mut edit = control.edit();
    let counter = edit
        .add_processor("counter", FrameCounter::default())
        .unwrap();
    to_device(&mut edit, counter.id());
    edit.commit().unwrap();

    let mut output = render(&mut engine, 100, 7);
    let mut added = Vec::new();
    for number in 0..9 {
        let mut edit = control.edit();
        let node = edit
            .add_processor(&format!("constant-{number}"), Constant(1_000.0))
            .unwrap();
        to_device(&mut edit, node.id());
        edit.commit().unwrap();
        added.push(node);
        output.extend(render(&mut engine, 100, 7));
    }
    for node in &added {
        let mut edit = control.edit();
        edit.remove_processor(node.id()).unwrap();
        edit.commit().unwrap();
        control.poll();
        // A removed processor's handle is dead, and its slot is free for the next one.
        assert_eq!(
            control.update(*node, 0.0),
            Err(GraphError::UnknownNode(node.id()))
        );
        output.extend(render(&mut engine, 100, 7));
    }

    // The counter never restarted: every frame is its count plus the constants alive then.
    for (frame, sample) in output.iter().enumerate() {
        let edits_done = frame / 100;
        let constants = if edits_done <= 9 {
            edits_done
        } else {
            18 - edits_done
        };
        assert_eq!(
            *sample,
            frame as f32 + 1_000.0 * constants as f32,
            "frame {frame}"
        );
    }
}

thread_local! {
    static INSIDE_PROCESS_BLOCK: Cell<bool> = const { Cell::new(false) };
    static DROPS: Cell<u32> = const { Cell::new(0) };
    static DROPS_INSIDE_PROCESS_BLOCK: Cell<u32> = const { Cell::new(0) };
}

/// Records where it was dropped.
struct DropProbe;

impl Drop for DropProbe {
    fn drop(&mut self) {
        DROPS.set(DROPS.get() + 1);
        if INSIDE_PROCESS_BLOCK.get() {
            DROPS_INSIDE_PROCESS_BLOCK.set(DROPS_INSIDE_PROCESS_BLOCK.get() + 1);
        }
    }
}

struct Snapshot {
    value: f32,
    _probe: DropProbe,
}

fn snapshot(value: f32) -> Arc<Snapshot> {
    Arc::new(Snapshot {
        value,
        _probe: DropProbe,
    })
}

/// A tiny stand-in for the arrangement processor: it plays from an immutable snapshot that
/// the control side replaces by message.
struct SnapshotReader {
    snapshot: Arc<Snapshot>,
    _probe: DropProbe,
}

impl Processor for SnapshotReader {
    type Update = Arc<Snapshot>;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Arc<Snapshot>) {
        std::mem::swap(&mut self.snapshot, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        context.audio_outputs.get(OUTPUT).fill(self.snapshot.value);
    }
}

fn render_watching_drops(engine: &mut Engine, frames: usize) -> Vec<f32> {
    INSIDE_PROCESS_BLOCK.set(true);
    let output = render(engine, frames, 100);
    INSIDE_PROCESS_BLOCK.set(false);
    output
}

#[test]
fn removed_processors_and_old_snapshots_are_dropped_on_the_control_side() {
    let (mut control, mut engine) = mono();
    let mut edit = control.edit();
    let reader = SnapshotReader {
        snapshot: snapshot(1.0),
        _probe: DropProbe,
    };
    let reader = edit.add_processor("reader", reader).unwrap();
    to_device(&mut edit, reader.id());
    edit.commit().unwrap();
    assert_eq!(render_watching_drops(&mut engine, 100), [1.0; 100]);

    // Replace the snapshot. The audio thread swaps it in and hands the old one back.
    control.update(reader, snapshot(2.0)).unwrap();
    assert_eq!(render_watching_drops(&mut engine, 100), [2.0; 100]);
    assert_eq!(DROPS.get(), 0, "the old snapshot waits in the return ring");
    control.poll();
    assert_eq!(DROPS.get(), 1, "poll dropped the old snapshot");

    // Remove the processor. It comes back with its current snapshot, and so does the schedule.
    let mut edit = control.edit();
    edit.remove_processor(reader.id()).unwrap();
    edit.commit().unwrap();
    assert_eq!(render_watching_drops(&mut engine, 100), [0.0; 100]);
    assert_eq!(DROPS.get(), 1);
    control.poll();
    assert_eq!(
        DROPS.get(),
        3,
        "poll dropped the processor and its snapshot"
    );
    assert_eq!(DROPS_INSIDE_PROCESS_BLOCK.get(), 0);
}

/// Breaks the realtime rules on purpose.
struct Allocating;

impl Processor for Allocating {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let allocated = std::hint::black_box(vec![1.0_f32; context.frames]);
        context
            .audio_outputs
            .get(OUTPUT)
            .copy_from_slice(&allocated);
    }
}

#[test]
#[ignore = "aborts under the realtime sanitizer. Run by the test below."]
fn allocate_inside_process() {
    let (mut control, mut engine) = mono();
    let mut edit = control.edit();
    let node = edit.add_processor("allocating", Allocating).unwrap();
    to_device(&mut edit, node.id());
    edit.commit().unwrap();
    assert_eq!(render(&mut engine, 10, 10), [1.0; 10]);
}

/// Without this, a sanitizer that is silently off would look like a clean run.
#[test]
fn the_realtime_sanitizer_catches_an_allocation_when_enabled() {
    if std::env::var_os("RTSAN_ENABLE").is_none() {
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "allocate_inside_process", "--ignored"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "the allocation went unnoticed");
    assert!(stderr.contains("RealtimeSanitizer"), "{stderr}");
}
