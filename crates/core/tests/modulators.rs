//! The envelope curves and the LFO shapes and tempo sync, as a modulation source uses them.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, Envelope, EnvelopeCurves, EnvelopeStage,
    EnvelopeState, Lfo, LfoShape, Ports, PrepareConfig, ProcessContext, Processor, Tempo,
    TempoChange, TempoMap, Ticks, TimeSignature,
};

const RATE: f32 = 48_000.0;

/// Frames from now until `done` holds, the first frame counted as 1.
fn frames_until(
    state: &mut EnvelopeState,
    envelope: &Envelope,
    done: impl Fn(&EnvelopeState) -> bool,
) -> usize {
    (1..10_000_000)
        .find(|_| {
            state.next(envelope);
            done(state)
        })
        .unwrap_or(0)
}

fn curves(curve: f32) -> EnvelopeCurves {
    EnvelopeCurves {
        attack: curve,
        decay: curve,
        release: curve,
    }
}

#[test]
fn every_curve_reaches_the_end_of_each_stage_in_its_time() {
    let (attack, decay, sustain, release) = (0.05, 0.4, 0.3, 0.2);
    for curve in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let envelope = Envelope::curved(attack, decay, sustain, release, curves(curve), RATE);
        let mut state = EnvelopeState::IDLE;
        state.start();
        let full = frames_until(&mut state, &envelope, |state| state.level >= 1.0);
        let expected = (attack * RATE).round() as usize;
        assert!(full.abs_diff(expected) <= 1, "attack at {curve}: {full}");
        // The decay stops on the sustain, where the analog decay only comes near it.
        let settled = frames_until(&mut state, &envelope, |state| {
            state.level == f64::from(sustain)
        });
        let expected = (decay * RATE).round() as usize;
        assert!(
            settled.abs_diff(expected) <= 1,
            "decay at {curve}: {settled}"
        );
        for _ in 0..100 {
            assert_eq!(state.next(&envelope), f64::from(sustain));
        }
        state.level = 1.0;
        state.release();
        let silent = frames_until(&mut state, &envelope, EnvelopeState::is_idle);
        let expected = (release * RATE).round() as usize;
        assert!(
            silent.abs_diff(expected) <= 1,
            "release at {curve}: {silent}"
        );
    }
}

#[test]
fn curve_0_is_straight_to_the_eye_and_1_is_fast_at_first() {
    let level_after = |curve: f32, frames: usize| {
        let envelope = Envelope::curved(0.1, 0.1, 0.0, 0.1, curves(curve), RATE);
        let mut state = EnvelopeState::IDLE;
        state.start();
        (0..frames).map(|_| state.next(&envelope)).last().unwrap()
    };
    // Half and a quarter of the 4800 frames of the attack.
    assert!((level_after(0.0, 2_400) - 0.5).abs() < 2e-4);
    assert!((level_after(0.0, 1_200) - 0.25).abs() < 2e-4);
    assert!(level_after(0.5, 2_400) > 0.58);
    assert!(level_after(1.0, 2_400) > 0.95);
    // Past the attack, a curve 0 decay to no sustain is halfway down half its time later.
    assert!((level_after(0.0, 4_800 + 2_400) - 0.5).abs() < 1e-3);
}

#[test]
fn a_release_of_curve_1_is_the_release_of_the_analog_shape() {
    let analog = Envelope::new(0.01, 0.2, 0.5, 0.3, RATE);
    let curved = Envelope::curved(0.01, 0.2, 0.5, 0.3, curves(1.0), RATE);
    let release = |envelope: &Envelope| {
        let mut state = EnvelopeState {
            stage: EnvelopeStage::Release,
            level: 0.8,
        };
        (0..20_000)
            .map(|_| state.next(envelope))
            .collect::<Vec<_>>()
    };
    assert_eq!(release(&analog), release(&curved));
}

#[test]
fn a_raised_sustain_is_reached_from_below_and_held() {
    let low = Envelope::curved(0.001, 0.1, 0.2, 0.1, curves(0.0), RATE);
    let high = Envelope::curved(0.001, 0.1, 0.6, 0.1, curves(0.0), RATE);
    let mut state = EnvelopeState::IDLE;
    state.start();
    frames_until(&mut state, &low, |state| state.level == f64::from(0.2_f32));
    let levels: Vec<_> = (0..10_000).map(|_| state.next(&high)).collect();
    assert!(levels.windows(2).all(|pair| pair[1] >= pair[0]));
    assert_eq!(levels.last(), Some(&f64::from(0.6_f32)));
}

/// The value of `shape` at each quarter of the first cycle of a 1 Hz LFO.
fn quarters_of(shape: LfoShape) -> [f32; 4] {
    let mut lfo = Lfo::default();
    [0; 4].map(|_| {
        let value = lfo.value(shape, 0.0);
        lfo.advance(12_000, 1.0, RATE);
        value
    })
}

#[test]
fn each_shape_has_its_values_at_the_quarters_of_a_cycle() {
    for (shape, expected) in [
        (LfoShape::Sine, [0.0, 1.0, 0.0, -1.0]),
        (LfoShape::Triangle, [0.0, 1.0, 0.0, -1.0]),
        (LfoShape::SawUp, [-1.0, -0.5, 0.0, 0.5]),
        (LfoShape::SawDown, [1.0, 0.5, 0.0, -0.5]),
        (LfoShape::Square, [1.0, 1.0, -1.0, -1.0]),
    ] {
        let values = quarters_of(shape);
        for (value, expected) in values.iter().zip(expected) {
            assert!((value - expected).abs() < 1e-6, "{shape:?}: {values:?}");
        }
    }
    // An offset is the same shape behind: an eighth behind a quarter is an eighth in.
    let mut lfo = Lfo::default();
    lfo.advance(12_000, 1.0, RATE);
    assert!((lfo.value(LfoShape::Triangle, 0.125) - 0.5).abs() < 1e-6);
    assert!((lfo.value(LfoShape::SawUp, 0.5) - 0.5).abs() < 1e-6);
}

/// The sample and hold level of each of the first `cycles` cycles, checked to hold inside
/// each cycle. Steps of an eighth cycle, which add up exactly.
fn held_levels(seed: u32, cycles: usize) -> Vec<f32> {
    let mut lfo = Lfo::seeded(seed);
    (0..cycles)
        .map(|_| {
            let level = lfo.value(LfoShape::SampleAndHold, 0.0);
            for _ in 0..7 {
                lfo.advance(1, 1.0, 8.0);
                assert_eq!(lfo.value(LfoShape::SampleAndHold, 0.0), level);
            }
            lfo.advance(1, 1.0, 8.0);
            level
        })
        .collect()
}

#[test]
fn sample_and_hold_is_the_same_for_the_same_seed_and_holds_through_a_cycle() {
    let levels = held_levels(7, 1_000);
    assert_eq!(levels, held_levels(7, 1_000));
    assert_ne!(levels, held_levels(8, 1_000));
    assert!(levels.iter().all(|level| (-1.0..1.0).contains(level)));
    // They spread over the whole range, and follow no simple pattern.
    let mean = levels.iter().sum::<f32>() / levels.len() as f32;
    assert!(mean.abs() < 0.1, "{mean}");
    assert!(levels.iter().any(|level| *level < -0.9));
    assert!(levels.iter().any(|level| *level > 0.9));
    assert!(levels.windows(2).all(|pair| pair[0] != pair[1]));
    // A cycle behind is the level of the cycle before.
    let mut lfo = Lfo::seeded(7);
    lfo.advance(3, 1.0, 2.0);
    assert_eq!(lfo.value(LfoShape::SampleAndHold, 0.0), levels[1]);
    assert_eq!(lfo.value(LfoShape::SampleAndHold, 1.0), levels[0]);
}

const OUTPUT: AudioOutput = AudioOutput::new(0);

/// A saw up LFO synced to one quarter note, written out every frame.
struct SyncedSaw(Lfo);

impl Processor for SyncedSaw {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let hz = self.0.sync(&context.transport, 1.0);
        let [left, _] = context.audio_outputs.get(OUTPUT);
        for sample in left.iter_mut() {
            *sample = self.0.value(LfoShape::SawUp, 0.0);
            self.0.advance(1, hz, RATE);
        }
    }
}

/// A processor that says its output is late, so that the engine plays everything else that
/// much ahead, and after a play waits for it: the saw starts inside a block.
struct Late;

impl Processor for Late {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn latency(&self) -> u32 {
        LATE_FRAMES as u32
    }

    fn process(&mut self, _: &mut ProcessContext<'_>) {}
}

const LATE_FRAMES: usize = 100;

/// Renders the synced saw at 120 bpm, then 90 bpm from the third quarter note: 100 frames
/// stopped, then playing, in device buffers of `buffer` frames. With `late`, the project waits
/// [`LATE_FRAMES`] after the play.
fn render_synced_saw(buffer: usize, late: bool) -> Vec<f32> {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(RATE as u32, 1));
    let mut edit = control.edit();
    let saw = edit
        .add_processor("saw", SyncedSaw(Lfo::default()))
        .unwrap();
    edit.connect(Connection::to_device(saw.id(), OUTPUT, 0))
        .unwrap();
    if late {
        let late = edit.add_processor("late", Late).unwrap();
        edit.connect(Connection::to_device(late.id(), OUTPUT, 0))
            .unwrap();
    }
    edit.commit().unwrap();
    let changes = [(0, 120.0), (1920, 90.0)].map(|(tick, bpm)| TempoChange {
        tick: Ticks(tick),
        bpm: Tempo::from_bpm(bpm).unwrap(),
    });
    control.set_tempo_map(TempoMap::new(TimeSignature::default(), changes.to_vec()).unwrap());
    let mut rendered = vec![0.0; 100 + 150_000];
    let (stopped, playing) = rendered.split_at_mut(100);
    engine.process_block(stopped);
    control.play();
    for block in playing.chunks_mut(buffer) {
        engine.process_block(block);
    }
    rendered
}

#[test]
fn a_synced_lfo_runs_at_the_tempo_and_starts_its_cycles_on_the_beat() {
    for (buffer, late) in [
        (64, false),
        (480, false),
        (513, false),
        (64, true),
        (513, true),
    ] {
        let rendered = render_synced_saw(buffer, late);
        // Stopped it runs free, at the 2 Hz of a quarter note at 120 bpm.
        let expected = 2.0 * (99.0 * 2.0 / RATE) - 1.0;
        assert!((rendered[99] - expected).abs() < 1e-4, "{}", rendered[99]);
        // Playing, a cycle starts on every beat: 24000 frames at 120 bpm, then 32000. Also
        // in the block where the project starts, after the wait for a late processor.
        let played = &rendered[100 + if late { LATE_FRAMES } else { 0 }..];
        for frame in (0..buffer).chain((0..played.len()).step_by(97)) {
            let cycles = if frame < 48_000 {
                frame as f64 / 24_000.0
            } else {
                2.0 + (frame - 48_000) as f64 / 32_000.0
            };
            let phase = cycles.fract();
            // Right on a beat the saw may be at either end. In the block where the tempo
            // changes it keeps the rate of the tempo where the block started.
            let on_a_beat = !(1e-3..1.0 - 1e-3).contains(&phase);
            if on_a_beat || (48_000..48_000 + buffer).contains(&frame) {
                continue;
            }
            let expected = (2.0 * phase - 1.0) as f32;
            let value = played[frame];
            assert!(
                (value - expected).abs() < 1e-3,
                "buffer {buffer}, late {late}, frame {frame}: {value}, not {expected}"
            );
        }
    }
}
