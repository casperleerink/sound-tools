//! Latency compensation in the engine, through the public API only, rendered offline.
//!
//! Two chains play the same beats into the device, one of them through a processor that
//! reports latency. What reaches the device must line up to the frame, whatever the latency
//! is and when it changes. See ARCHITECTURE.md, "Latency compensation".

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sound_core::{
    AudioInput, AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventInput,
    EventOutput, Node, Ports, PrepareConfig, ProcessContext, Processor, Ticks,
};

/// 120 bpm at 48 kHz: 25 frames a tick.
const FRAMES_PER_TICK: usize = 25;
/// A beat every 48 ticks, 1200 frames.
const INTERVAL: u64 = 48;
const CHANNELS: usize = 4;
/// The longest delay the test processor can hold.
const MAX_DELAY: usize = 4096;

#[derive(Copy, Clone)]
struct Beat(u64);

const BEATS_OUT: EventOutput<Beat> = EventOutput::new(0);
const BEATS_IN: EventInput<Beat> = EventInput::new(0);
const AUDIO_IN: AudioInput = AudioInput::new(0);
const AUDIO_OUT: AudioOutput = AudioOutput::new(0);

/// Emits a beat at every multiple of [`INTERVAL`] ticks, from the transport alone, and notes
/// the last tick the device played as the transport told it, for the test to read.
struct Beats {
    heard_tick: Arc<AtomicU64>,
    started_at: Arc<AtomicU64>,
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
        self.heard_tick
            .store(transport.heard_tick.0, Ordering::Relaxed);
        self.started_at
            .store(transport.tick_range.start.0, Ordering::Relaxed);
        let mut tick = transport.tick_range.start.0.next_multiple_of(INTERVAL);
        while tick < transport.tick_range.end.0 {
            if let Some(offset) = transport.offset_of(Ticks(tick)) {
                context
                    .event_outputs
                    .push(BEATS_OUT, offset, Beat(tick / INTERVAL));
            }
            tick += INTERVAL;
        }
    }
}

/// Writes `beat + 1` at the frame each beat arrives on.
struct Impulses;

impl Processor for Impulses {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_input(BEATS_IN).audio_output(AUDIO_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left, right] = context.audio_outputs.get(AUDIO_OUT);
        for timed in context.event_inputs.get(BEATS_IN) {
            if let Some(sample) = left.get_mut(timed.offset) {
                *sample += (timed.event.0 + 1) as f32;
            }
        }
        right.copy_from_slice(left);
    }
}

/// Delays its input by as many frames as it says its latency is. The update is a new latency,
/// which starts the delay line empty, as a plugin that is started again does.
struct Delay {
    latency: u32,
    line: Vec<f32>,
    written: usize,
}

impl Delay {
    fn new(latency: u32) -> Self {
        Self {
            latency,
            line: vec![0.0; MAX_DELAY],
            written: 0,
        }
    }
}

impl Processor for Delay {
    type Update = u32;

    fn ports(&self) -> Ports {
        Ports::new().audio_input(AUDIO_IN).audio_output(AUDIO_OUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, latency: &mut u32) {
        self.latency = *latency;
        self.line.fill(0.0);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [input, _] = context.audio_inputs.get(AUDIO_IN);
        let [left, right] = context.audio_outputs.get(AUDIO_OUT);
        let delay = self.latency as usize;
        for (sample, played) in left.iter_mut().zip(input) {
            self.line[self.written % MAX_DELAY] = *played;
            *sample = self.line[(self.written + MAX_DELAY - delay) % MAX_DELAY];
            self.written += 1;
        }
        right.copy_from_slice(left);
    }

    fn latency(&self) -> u32 {
        self.latency
    }
}

/// Two chains of beats: `plain` to device channels 0 and 1, and `delayed` through a [`Delay`]
/// to channels 2 and 3.
struct Chains {
    control: EngineControl,
    engine: Engine,
    delay: Node<Delay>,
    heard_tick: Arc<AtomicU64>,
    started_at: Arc<AtomicU64>,
    /// What the device played: channel 0, and channel 2.
    plain: Vec<f32>,
    delayed: Vec<f32>,
    /// Every frame the engine made, preroll and all, channel 0.
    everything: Vec<f32>,
}

impl Chains {
    fn new(latency: u32) -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(48_000, CHANNELS));
        let heard_tick = Arc::new(AtomicU64::new(0));
        let started_at = Arc::new(AtomicU64::new(0));
        let mut edit = control.edit();
        let beats = |name: &str, edit: &mut sound_core::Edit<'_>, probe: bool| {
            let (heard_tick, started_at) = match probe {
                true => (heard_tick.clone(), started_at.clone()),
                false => (Arc::default(), Arc::default()),
            };
            edit.add_processor(
                name,
                Beats {
                    heard_tick,
                    started_at,
                },
            )
            .unwrap()
        };
        let plain_beats = beats("a-beats", &mut edit, false);
        let plain = edit.add_processor("a-impulses", Impulses).unwrap();
        let delayed_beats = beats("b-beats", &mut edit, true);
        let delayed = edit.add_processor("b-impulses", Impulses).unwrap();
        let delay = edit.add_processor("b-delay", Delay::new(latency)).unwrap();
        for (from, to) in [(plain_beats, plain), (delayed_beats, delayed)] {
            edit.connect(Connection::new(from.id(), BEATS_OUT, to.id(), BEATS_IN))
                .unwrap();
        }
        edit.connect(Connection::new(
            delayed.id(),
            AUDIO_OUT,
            delay.id(),
            AUDIO_IN,
        ))
        .unwrap();
        edit.connect(Connection::to_device(plain.id(), AUDIO_OUT, 0))
            .unwrap();
        edit.connect(Connection::to_device(delay.id(), AUDIO_OUT, 2))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            delay,
            heard_tick,
            started_at,
            plain: Vec::new(),
            delayed: Vec::new(),
            everything: Vec::new(),
        }
    }

    /// Renders `frames` more frames in device buffers of 512, and keeps what a render keeps:
    /// everything but the frames the device played while it waited after a play or a seek.
    fn render(&mut self, frames: usize) {
        let mut preroll = self.engine.preroll_frames();
        let mut left = frames;
        while left > 0 {
            let size = left.min(512);
            let mut buffer = vec![0.0; size * CHANNELS];
            self.engine.process_block(&mut buffer);
            let now = self.engine.preroll_frames();
            self.control.poll().unwrap();
            let waited = (now - preroll) as usize;
            preroll = now;
            for (index, frame) in buffer.chunks(CHANNELS).enumerate() {
                self.everything.push(frame[0]);
                if index >= waited {
                    self.plain.push(frame[0]);
                    self.delayed.push(frame[2]);
                }
            }
            left -= size;
        }
    }
}

/// Every frame that holds an impulse, with its value.
fn impulses(samples: &[f32]) -> Vec<(usize, f32)> {
    let samples = samples.iter().copied().enumerate();
    samples.filter(|(_, sample)| *sample != 0.0).collect()
}

/// The frames the beats land on from tick 0, with their values.
fn expected(count: u64) -> Vec<(usize, f32)> {
    (0..count)
        .map(|beat| {
            let frame = beat as usize * INTERVAL as usize * FRAMES_PER_TICK;
            (frame, (beat + 1) as f32)
        })
        .collect()
}

#[test]
fn a_chain_with_latency_reaches_the_device_in_time_with_one_without() {
    for latency in [1, 63, 64, 300, 1200, 3000] {
        let mut chains = Chains::new(latency);
        chains.control.play();
        chains.render(latency as usize + 6 * 1200);
        let status = chains.control.poll().unwrap();
        assert_eq!(status.latency, u64::from(latency));
        assert_eq!(chains.engine.preroll_frames(), u64::from(latency));
        // Tick 0 is the first frame after the wait, on both chains, and every beat lands on
        // the frame of its tick: nothing is early, late or missing.
        assert_eq!(impulses(&chains.plain), expected(6), "latency {latency}");
        assert_eq!(impulses(&chains.delayed), expected(6), "latency {latency}");
        // Before that the device played nothing of the timeline.
        let waited = &chains.everything[..latency as usize];
        assert!(waited.iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn without_latency_nothing_waits_and_nothing_moves() {
    let mut chains = Chains::new(0);
    chains.control.play();
    chains.render(6 * 1200);
    let status = chains.control.poll().unwrap();
    assert_eq!((status.latency, chains.engine.preroll_frames()), (0, 0));
    assert_eq!(chains.everything, chains.plain);
    assert_eq!(impulses(&chains.plain), expected(6));
    assert_eq!(impulses(&chains.delayed), expected(6));
}

#[test]
fn the_playhead_waits_where_playback_starts_until_the_slowest_chain_is_heard() {
    let mut chains = Chains::new(300);
    chains.render(64);
    chains.control.seek(Ticks(96));
    chains.control.play();
    chains.render(256);
    let status = chains.control.poll().unwrap();
    // 256 frames played, 300 of latency: the device has not reached tick 96 yet.
    assert_eq!(status.playhead_tick, Ticks(96));
    assert!(status.playing);
    chains.render(256);
    let status = chains.control.poll().unwrap();
    // 212 frames past the wait.
    assert_eq!(status.playhead_frame.0, 96 * 25 + 212);
}

#[test]
fn a_seek_plays_from_where_it_went_on_every_chain_and_nothing_before_it() {
    let mut chains = Chains::new(500);
    chains.control.play();
    chains.render(1000);
    let kept = chains.plain.len();
    chains.control.seek(Ticks(4 * INTERVAL));
    chains.render(4 * 1200);
    // Right after the seek, beat 4 is the first thing on both chains, on the same frame.
    let after = |samples: &[f32]| impulses(&samples[kept..]);
    let after_the_seek: Vec<(usize, f32)> = (4..8)
        .map(|beat| ((beat - 4) * 1200, beat as f32 + 1.0))
        .collect();
    assert_eq!(after(&chains.plain), after_the_seek);
    assert_eq!(after(&chains.delayed), after_the_seek);
}

#[test]
fn a_play_after_a_pause_waits_again_and_skips_nothing() {
    let mut chains = Chains::new(700);
    chains.control.play();
    // 700 frames of wait, then the device plays frames 0 to 1100 of the project.
    chains.render(1800);
    chains.control.pause();
    chains.render(2000);
    // A play from rest is a seek to where the project stands: another wait of 700 frames,
    // which a render leaves out, and then the project from frame 1100 on.
    chains.control.play();
    chains.render(6000);
    let resumed = |beat: usize| (3100 + beat * 1200 - 1100, (beat + 1) as f32);
    let after_the_pause: Vec<_> = (1..6).map(resumed).collect();
    let mut plain = vec![(0, 1.0)];
    plain.extend(&after_the_pause);
    assert_eq!(impulses(&chains.plain), plain);
    // The chain with latency had already played beat 2 into its delay when the pause came,
    // so that one rings out during the pause, 100 frames in. From the play on, it is the
    // same as the other chain: nothing skipped.
    let mut delayed = vec![(0, 1.0), (1200, 2.0)];
    delayed.extend(&after_the_pause);
    assert_eq!(impulses(&chains.delayed), delayed);
}

#[test]
fn a_latency_that_changes_while_playing_is_in_time_again_from_the_next_beat() {
    let mut chains = Chains::new(200);
    chains.control.play();
    // 200 frames of wait, then frames 0 to 2800.
    chains.render(3000);
    // Longer, then shorter. The chain whose latency moved sees a jump and plays on from where
    // it now has to be; the other chain is not touched at all.
    for latency in [900, 50] {
        chains.control.update(chains.delay, latency).unwrap();
        chains.render(3600);
    }
    let status = chains.control.poll().unwrap();
    assert_eq!(status.latency, 50);
    // Only the first wait: a latency change is not a seek.
    assert_eq!(chains.engine.preroll_frames(), 200);
    // The plain chain played every beat on its frame the whole time.
    assert_eq!(impulses(&chains.plain), expected(9));
    // The delayed one too, but for the beat at frame 3600: at frame 2800 its latency grew by
    // 700 frames, so the beats before it had to be played 700 frames earlier than they were,
    // and the one that falls in that stretch is not played at all. The shorter latency at
    // 6400 plays again what its delay line held when it was started again, so nothing is lost
    // there.
    let mut delayed = expected(9);
    delayed.retain(|(frame, _)| *frame != 3600);
    assert_eq!(impulses(&chains.delayed), delayed);
}

#[test]
fn a_processor_ahead_of_the_device_is_told_what_the_device_plays() {
    let mut chains = Chains::new(1250);
    chains.control.play();
    chains.render(1250 + 2500);
    // The device is at tick 100; the processor before the delay is 1250 frames, 50 ticks,
    // ahead of it.
    let heard = chains.heard_tick.load(Ordering::Relaxed);
    let started = chains.started_at.load(Ordering::Relaxed);
    assert_eq!(started - heard, 50);
    // The last device buffer is 166 frames, so the last block is 38: it began at frame 2462
    // of the project, which is tick 98.48, and a frame belongs to the tick at or after it.
    assert_eq!(heard, 99);
}

/// A play from rest waits for the latency, and the processors see that as a jump, but it is no
/// seek: the engine status counts none. A take or a view that ends on a jump must not end
/// because play was pressed.
#[test]
fn a_play_from_rest_with_latency_counts_no_jump() {
    let mut chains = Chains::new(700);
    chains.render(512);
    chains.control.play();
    chains.render(2048);
    chains.control.pause();
    chains.render(512);
    chains.control.play();
    chains.render(2048);
    let status = chains.control.poll().unwrap();
    assert_eq!(status.jumps, 0);
    assert_eq!(chains.engine.preroll_frames(), 1400);
    // A seek still counts.
    chains.control.seek(Ticks(0));
    chains.render(512);
    assert_eq!(chains.control.poll().unwrap().jumps, 1);
}

/// A tempo map change while playing keeps the tick the device plays. A chain ahead of the
/// device by 3000 frames, 120 ticks at 120 bpm, would land 240 ticks ahead at 240 bpm and
/// skip beats, or 60 ticks ahead at 60 bpm and play some twice. Every beat plays once, on
/// both chains. After the faster tempo the beats the chain ahead would have skipped come at
/// once, late, and add up on one frame, so the delayed chain is checked by its sum.
#[test]
fn a_tempo_change_while_playing_plays_every_beat_once_on_a_chain_ahead() {
    for bpm in [240.0, 60.0] {
        let mut chains = Chains::new(3000);
        chains.control.play();
        chains.render(3000 + 5000);
        let changes = vec![sound_core::TempoChange {
            tick: Ticks(0),
            bpm: sound_core::Tempo::from_bpm(bpm).unwrap(),
        }];
        let map = sound_core::TempoMap::new(sound_core::TimeSignature::default(), changes);
        chains.control.set_tempo_map(map.unwrap());
        chains.render(40_000);
        let beats = |samples: &[f32]| -> Vec<u64> {
            impulses(samples)
                .iter()
                .map(|(_, value)| *value as u64)
                .collect()
        };
        // The chain without latency: every beat once, in order.
        let plain = beats(&chains.plain);
        let in_order: Vec<u64> = (1..=plain.len() as u64).collect();
        assert_eq!(plain, in_order, "{bpm} bpm, plain");
        assert!(plain.len() > 10, "{bpm} bpm: {plain:?}");
        // The chain ahead: beats 1 to its last one, each exactly once.
        let delayed = beats(&chains.delayed);
        let last = *delayed.last().unwrap();
        let total: u64 = delayed.iter().sum();
        assert_eq!(total, last * (last + 1) / 2, "{bpm} bpm: {delayed:?}");
        assert!(
            last + 1 >= plain.len() as u64,
            "{bpm} bpm: {delayed:?} {plain:?}"
        );
    }
}
