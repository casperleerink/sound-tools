//! The transport and the clock inside the engine, through the public API only, rendered offline.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use proptest::prelude::*;
use sound_core::{
    AudioOutput, Clock, Connection, Engine, EngineConfig, EngineControl, EventInput, EventOutput,
    Frames, Ports, PrepareConfig, ProcessContext, Processor, Tempo, TempoChange, TempoMap, Ticks,
    TimeSignature,
};

/// The count of the beat that fired: tick / interval.
#[derive(Copy, Clone)]
struct Beat(u64);

const BEATS_OUT: EventOutput<Beat> = EventOutput::new(0);
const BEATS_IN: EventInput<Beat> = EventInput::new(0);
const OUTPUT: AudioOutput = AudioOutput::new(0);

/// Emits a [`Beat`] at every multiple of `interval` ticks, the way a timeline-driven processor
/// should: from `context.transport` alone, with no rounding of its own. It also checks what
/// the transport promises and counts every broken promise.
struct Beats {
    interval: u64,
    previous_end: Option<Ticks>,
    broken_promises: Arc<AtomicU64>,
}

impl Processor for Beats {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(BEATS_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        let covered = transport.frame_range.end.0 - transport.frame_range.start.0;
        let continues = self
            .previous_end
            .is_none_or(|end| end == transport.tick_range.start);
        let kept = if transport.playing {
            covered == context.frames as u64 && (continues || transport.jumped)
        } else {
            covered == 0 && transport.tick_range.is_empty()
        };
        if !kept {
            self.broken_promises.fetch_add(1, Ordering::Relaxed);
        }
        self.previous_end = Some(transport.tick_range.end);

        let mut tick = transport.tick_range.start.0.next_multiple_of(self.interval);
        while tick < transport.tick_range.end.0 {
            match transport.offset_of(Ticks(tick)) {
                Some(offset) if offset < context.frames => {
                    let beat = Beat(tick / self.interval);
                    context.event_outputs.push(BEATS_OUT, offset, beat);
                }
                _ => {
                    self.broken_promises.fetch_add(1, Ordering::Relaxed);
                }
            }
            tick += self.interval;
        }
    }
}

/// Adds `beat + 1` to the frame each beat arrives at, so the render shows which beat fired
/// where, and a beat that fired twice shows as a wrong value or a second impulse.
struct Impulses;

fn impulse(beat: u64) -> f32 {
    (beat % 1000 + 1) as f32
}

impl Processor for Impulses {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_input(BEATS_IN).audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let output = context.audio_outputs.get(OUTPUT);
        for timed in context.event_inputs.get(BEATS_IN) {
            if let Some(sample) = output.get_mut(timed.offset) {
                *sample += impulse(timed.event.0);
            }
        }
    }
}

struct BeatEngine {
    control: EngineControl,
    engine: Engine,
    broken_promises: Arc<AtomicU64>,
    rendered: Vec<f32>,
}

impl BeatEngine {
    fn new(sample_rate: u32, interval: u64) -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(sample_rate, 1));
        let broken_promises = Arc::new(AtomicU64::new(0));
        let mut edit = control.edit();
        let beats = Beats {
            interval,
            previous_end: None,
            broken_promises: broken_promises.clone(),
        };
        let beats = edit.add_processor("beats", beats).unwrap();
        let impulses = edit.add_processor("impulses", Impulses).unwrap();
        edit.connect(Connection::new(
            beats.id(),
            BEATS_OUT,
            impulses.id(),
            BEATS_IN,
        ))
        .unwrap();
        edit.connect(Connection::to_device(impulses.id(), OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            broken_promises,
            rendered: Vec::new(),
        }
    }

    /// Renders `frames` more frames, in device buffers of the given sizes, used in turn.
    /// Gives the new part of `rendered`.
    fn render(&mut self, frames: usize, device_buffers: &[usize]) -> &[f32] {
        let end = self.rendered.len() + frames;
        self.rendered.resize(end, 0.0);
        let mut position = end - frames;
        for size in device_buffers.iter().cycle() {
            if position == end {
                break;
            }
            let next = (position + size).min(end);
            self.engine
                .process_block(&mut self.rendered[position..next]);
            position = next;
        }
        self.control.poll().unwrap();
        &self.rendered[end - frames..]
    }

    /// Every frame that holds an impulse, with its value.
    fn impulses(&self) -> Vec<(usize, f32)> {
        let samples = self.rendered.iter().copied().enumerate();
        samples.filter(|(_, sample)| *sample != 0.0).collect()
    }
}

fn bpm(value: f64) -> Tempo {
    Tempo::from_bpm(value).unwrap()
}

fn tempo_map(changes: &[(u64, f64)]) -> TempoMap {
    let changes = changes
        .iter()
        .map(|(tick, value)| TempoChange {
            tick: Ticks(*tick),
            bpm: bpm(*value),
        })
        .collect();
    TempoMap::new(TimeSignature::default(), changes).unwrap()
}

#[test]
fn quarter_notes_land_on_exact_engine_frames_for_every_device_buffer_size() {
    for device_buffer in [1, 63, 64, 65, 480, 513] {
        let mut beats = BeatEngine::new(48_000, 960);
        // 120 bpm, then 90 bpm from the third quarter note on.
        beats
            .control
            .set_tempo_map(tempo_map(&[(0, 120.0), (1920, 90.0)]));
        // Engine time runs before the project plays, so the two are not the same number.
        beats.render(100, &[device_buffer]);
        beats.control.play();
        beats.render(150_000, &[device_buffer]);

        // A quarter note is 24000 frames at 120 bpm and 32000 frames at 90 bpm.
        let expected = [0, 24_000, 48_000, 80_000, 112_000, 144_000];
        let expected: Vec<_> = (0..)
            .zip(expected)
            .map(|(beat, frame)| (100 + frame, impulse(beat)))
            .collect();
        assert_eq!(beats.impulses(), expected, "device buffer {device_buffer}");
        assert_eq!(beats.broken_promises.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn a_tempo_change_inside_a_block_moves_the_next_beat() {
    // Tick 1 is at frame 25. From there a tick lasts 50 frames, so tick 2 is at frame 75,
    // inside the second engine block, and tick 3 at frame 125.
    let mut beats = BeatEngine::new(48_000, 1);
    beats
        .control
        .set_tempo_map(tempo_map(&[(0, 120.0), (1, 60.0)]));
    beats.control.play();
    beats.render(130, &[130]);
    let frames: Vec<_> = beats.impulses().iter().map(|(frame, _)| *frame).collect();
    assert_eq!(frames, [0, 25, 75, 125]);
    assert_eq!(beats.broken_promises.load(Ordering::Relaxed), 0);
}

fn any_tempo_map() -> impl Strategy<Value = TempoMap> {
    let tempo = || (10_000_u32..=1_000_000).prop_map(|milli| bpm(f64::from(milli) / 1000.0));
    // Gaps from one tick up, so several tempo changes can fall inside one block.
    let later = prop::collection::vec((1_u64..3_000, tempo()), 0..6);
    (tempo(), later).prop_map(|(first, later)| {
        let mut tick = 0;
        let mut changes = vec![TempoChange {
            tick: Ticks(0),
            bpm: first,
        }];
        for (gap, bpm) in later {
            tick += gap;
            changes.push(TempoChange {
                tick: Ticks(tick),
                bpm,
            });
        }
        TempoMap::new(TimeSignature::default(), changes).unwrap()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Random tempo maps, a tempo map swap during playback, random device buffer sizes and a
    /// pause in the middle. Every beat fires exactly once, on the frame the clock gives.
    #[test]
    fn every_tick_belongs_to_exactly_one_block(
        first_map in any_tempo_map(),
        second_map in any_tempo_map(),
        sample_rate in prop::sample::select(vec![44_100_u32, 48_000, 96_000]),
        interval in 1_u64..400,
        device_buffers in prop::collection::vec(1_usize..700, 1..5),
        paused_frames in 0_usize..200,
        playing_frames in [0_usize..8_000, 0_usize..8_000, 0_usize..8_000],
    ) {
        let mut beats = BeatEngine::new(sample_rate, interval);
        beats.control.set_tempo_map(first_map.clone());
        let mut played = Vec::new();
        let mut paused = Vec::new();

        paused.extend_from_slice(beats.render(paused_frames, &device_buffers));
        beats.control.play();
        played.extend_from_slice(beats.render(playing_frames[0], &device_buffers));
        beats.control.set_tempo_map(second_map.clone());
        played.extend_from_slice(beats.render(playing_frames[1], &device_buffers));
        beats.control.pause();
        paused.extend_from_slice(beats.render(paused_frames, &device_buffers));
        beats.control.play();
        played.extend_from_slice(beats.render(playing_frames[2], &device_buffers));

        // The expected impulses, on a timeline of only the frames rendered while playing.
        let mut expected = vec![0.0_f32; played.len()];
        let mut expect = |clock: &Clock, ticks: std::ops::Range<Ticks>, start: usize| {
            let project_start = clock.frame_of(ticks.start).0;
            for tick in (ticks.start.0..ticks.end.0).filter(|tick| tick.is_multiple_of(interval)) {
                let frame = clock.frame_of(Ticks(tick)).0 - project_start;
                expected[start + frame as usize] += impulse(tick / interval);
            }
        };
        let first = Clock::new(first_map, sample_rate);
        let swapped_at = first.tick_at(Frames(playing_frames[0] as u64));
        expect(&first, Ticks(0)..swapped_at, 0);
        // The swap keeps the musical position: the second clock goes on at the same tick.
        let second = Clock::new(second_map, sample_rate);
        let after_swap = (playing_frames[1] + playing_frames[2]) as u64;
        let end = Frames(second.frame_of(swapped_at).0 + after_swap);
        expect(&second, swapped_at..second.tick_at(end), playing_frames[0]);

        prop_assert_eq!(played, expected);
        prop_assert!(paused.iter().all(|sample| *sample == 0.0));
        prop_assert_eq!(beats.broken_promises.load(Ordering::Relaxed), 0);
        let status = beats.control.poll().unwrap();
        prop_assert_eq!(status.playhead_frame, end);
        prop_assert_eq!(status.playhead_tick, second.tick_at(end));
        prop_assert_eq!(status.event_overflows, 0);
    }
}

const PLAYING: AudioOutput = AudioOutput::new(0);
const JUMPED: AudioOutput = AudioOutput::new(1);
const START_TICK: AudioOutput = AudioOutput::new(2);

/// Writes what it sees of the transport to three device channels.
struct Probe;

impl Processor for Probe {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_output(PLAYING)
            .audio_output(JUMPED)
            .audio_output(START_TICK)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        let values = [
            f32::from(u8::from(transport.playing)),
            f32::from(u8::from(transport.jumped)),
            transport.tick_range.start.0 as f32,
        ];
        let outputs = context
            .audio_outputs
            .get_many([PLAYING, JUMPED, START_TICK]);
        for (output, value) in outputs.into_iter().zip(values) {
            output.fill(value);
        }
    }
}

/// What the probe saw in one engine block.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Seen {
    playing: bool,
    jumped: bool,
    start_tick: u64,
}

fn seen(playing: bool, jumped: bool, start_tick: u64) -> Seen {
    Seen {
        playing,
        jumped,
        start_tick,
    }
}

fn probe_engine() -> (EngineControl, Engine) {
    let (mut control, engine) = Engine::new(EngineConfig::new(48_000, 3));
    let mut edit = control.edit();
    let probe = edit.add_processor("probe", Probe).unwrap();
    for (channel, port) in [PLAYING, JUMPED, START_TICK].into_iter().enumerate() {
        edit.connect(Connection::to_device(probe.id(), port, channel))
            .unwrap();
    }
    edit.commit().unwrap();
    (control, engine)
}

/// Renders whole engine blocks of 64 frames and gives what the probe saw in each.
fn render_blocks(engine: &mut Engine, blocks: usize) -> Vec<Seen> {
    let mut output = vec![0.0; blocks * 64 * 3];
    engine.process_block(&mut output);
    let blocks = output.chunks(64 * 3).map(|block| {
        let first = seen(block[0] == 1.0, block[1] == 1.0, block[2] as u64);
        for frame in block.chunks(3) {
            assert_eq!(
                seen(frame[0] == 1.0, frame[1] == 1.0, frame[2] as u64),
                first
            );
        }
        first
    });
    blocks.collect()
}

// At 48 kHz and 120 bpm a tick is 25 frames, so an engine block of 64 frames is 2.56 ticks.

#[test]
fn pause_holds_play_resumes_and_stop_returns_to_zero() {
    let (mut control, mut engine) = probe_engine();

    // Stopped at the start: engine time runs, the project position does not.
    assert_eq!(render_blocks(&mut engine, 2), [seen(false, false, 0); 2]);
    let status = control.poll().unwrap();
    assert_eq!((status.frames, status.playing), (128, false));
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(0), Ticks(0))
    );

    control.play();
    let blocks = render_blocks(&mut engine, 100);
    assert_eq!(blocks[0], seen(true, false, 0));
    assert_eq!(blocks[1], seen(true, false, 3));
    assert_eq!(blocks[99], seen(true, false, 254));
    let status = control.poll().unwrap();
    assert_eq!((status.frames, status.playing), (128 + 6400, true));
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(6400), Ticks(256))
    );

    control.pause();
    assert_eq!(
        render_blocks(&mut engine, 10),
        [seen(false, false, 256); 10]
    );
    let status = control.poll().unwrap();
    assert_eq!((status.frames, status.playing), (128 + 6400 + 640, false));
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(6400), Ticks(256))
    );

    control.play();
    assert_eq!(
        render_blocks(&mut engine, 2),
        [seen(true, false, 256), seen(true, false, 259)]
    );

    control.stop();
    assert_eq!(
        render_blocks(&mut engine, 3),
        [
            seen(false, true, 0),
            seen(false, false, 0),
            seen(false, false, 0)
        ]
    );
    let status = control.poll().unwrap();
    assert_eq!(
        (status.frames, status.playing),
        (128 + 6400 + 640 + 128 + 192, false)
    );
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(0), Ticks(0))
    );

    control.play();
    assert_eq!(render_blocks(&mut engine, 1), [seen(true, false, 0)]);
}

#[test]
fn seek_keeps_the_playing_state_and_flags_one_block() {
    let (mut control, mut engine) = probe_engine();

    control.seek(Ticks(9600));
    assert_eq!(
        render_blocks(&mut engine, 3),
        [
            seen(false, true, 9600),
            seen(false, false, 9600),
            seen(false, false, 9600)
        ]
    );
    let status = control.poll().unwrap();
    assert_eq!((status.playing, status.playhead_tick), (false, Ticks(9600)));

    control.play();
    assert_eq!(
        render_blocks(&mut engine, 2),
        [seen(true, false, 9600), seen(true, false, 9603)]
    );

    // Backwards during playback. The block after the seek starts exactly on the target.
    control.seek(Ticks(960));
    assert_eq!(
        render_blocks(&mut engine, 3),
        [
            seen(true, true, 960),
            seen(true, false, 963),
            seen(true, false, 966)
        ]
    );
    let status = control.poll().unwrap();
    assert_eq!(
        (status.playing, status.playhead_frame),
        (true, Frames(24_000 + 192))
    );
}

#[test]
fn a_tempo_map_swap_keeps_the_musical_position_and_the_old_clock_comes_back() {
    let (mut control, mut engine) = probe_engine();
    control.play();
    render_blocks(&mut engine, 750);
    let status = control.poll().unwrap();
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(48_000), Ticks(1920))
    );

    let old_clock = Arc::downgrade(control.clock());
    control.set_tempo_map(tempo_map(&[(0, 60.0)]));
    assert_eq!(control.clock().tempo_at(Ticks(0)), bpm(60.0));
    // The audio thread still holds the old clock.
    assert_eq!(old_clock.strong_count(), 1);

    // At 60 bpm a tick is 50 frames. Tick 1920 is now at frame 96000, and playback goes on
    // from tick 1920 with no jump.
    assert_eq!(
        render_blocks(&mut engine, 2),
        [seen(true, false, 1920), seen(true, false, 1922)]
    );
    // The audio thread let go of the old clock without dropping it: it waits in the return
    // ring until the control side polls.
    assert_eq!(old_clock.strong_count(), 1);
    let status = control.poll().unwrap();
    assert_eq!(old_clock.strong_count(), 0);
    assert_eq!(
        (status.playhead_frame, status.playhead_tick),
        (Frames(96_128), Ticks(1923))
    );
    assert!(status.playing);
}
