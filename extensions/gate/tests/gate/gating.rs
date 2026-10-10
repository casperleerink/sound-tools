//! The gate: down by the range under the threshold and untouched over it, open for the hold,
//! keyed by the sidechain, and the same render in any block size.

use gate::{GateState, static_gain_db};

use crate::support::{Rig, SAMPLE_RATE, Signal, decibels, dry, hits, peak, sine};

const SECOND: usize = SAMPLE_RATE as usize;

fn gate() -> GateState {
    GateState {
        threshold_db: -40.0,
        range_db: 24.0,
        ..GateState::default()
    }
}

/// A steady 1 kHz sine at -50 dBFS comes out down by the range, and one at -20 dBFS sample for
/// sample as it went in. At 1 kHz a sample falls on each peak, so the peak is the amplitude.
#[test]
fn a_sound_under_the_threshold_is_down_by_the_range_and_one_over_it_is_untouched() {
    for level_db in [-50.0, -20.0] {
        let amplitude = decibels(level_db);
        let mut rig = Rig::new(gate(), sine(1_000.0, amplitude));
        let output = rig.render(2 * SECOND);
        let settled = &output[SECOND..];
        let measured_db = 20.0 * (peak(settled) / amplitude).log10();
        let expected = static_gain_db(&gate(), level_db);
        assert!(
            (measured_db - expected).abs() < 1e-3,
            "{level_db} dB: {measured_db} dB, expected {expected}"
        );
        if expected == 0.0 {
            let input = dry(sine(1_000.0, amplitude), 2 * SECOND);
            assert_eq!(settled, &input[SECOND..]);
        }
    }
}

/// A constant at `level` until each frame, then the next one, in both channels.
fn steps<const N: usize>(parts: [(usize, f32); N]) -> Signal {
    let mut frame = 0;
    Box::new(move || {
        let level = parts.iter().find(|(until, _)| frame < *until);
        frame += 1;
        [level.map_or(0.0, |(_, level)| *level); 2]
    })
}

const LOUD: f32 = 0.5;
const AT: usize = SECOND / 2;
const MILLISECOND: usize = SECOND / 1_000;

fn quiet() -> f32 {
    decibels(-60.0)
}

/// A constant that steps from loud to under the threshold: the gate stays open, gain exactly
/// 1, while the loud part is in the detector (10 to 11 ms) and for the hold after, then closes.
#[test]
fn the_gate_stays_open_for_the_hold_after_the_level_falls() {
    for hold_ms in [0.0, 50.0] {
        let state = GateState {
            hold_ms,
            release_ms: 1.0,
            ..gate()
        };
        let signal = steps([(AT, LOUD), (SECOND, quiet())]);
        let output = Rig::new(state, signal).render(SECOND);
        let gain = |frame: usize| output[frame] / quiet();
        let open_until = AT - 1 + 10 * MILLISECOND + hold_ms as usize * MILLISECOND;
        assert_eq!(gain(open_until), 1.0, "hold {hold_ms}");
        let closed = gain(open_until + 12 * MILLISECOND);
        let floor = decibels(-24.0);
        assert!((closed - floor).abs() < 1e-3, "hold {hold_ms}: {closed}");
    }
}

/// Digital silence inside the hold does not close the gate: a quiet sound 20 ms after the
/// loud one still comes out untouched while a hold of 100 ms lasts.
#[test]
fn silence_inside_the_hold_keeps_the_gate_open() {
    let state = GateState {
        hold_ms: 100.0,
        release_ms: 1.0,
        ..gate()
    };
    let gap = 20 * MILLISECOND;
    let signal = steps([(AT, LOUD), (AT + gap, 0.0), (SECOND, quiet())]);
    let output = Rig::new(state, signal).render(SECOND);
    assert_eq!(output[AT + gap + 10 * MILLISECOND] / quiet(), 1.0);
}

/// The gate opens 63 % of the way from the floor in `attack_ms`, and closes 63 % of the way
/// in `release_ms` after its last open frame: a one-pole glide of that time constant.
#[test]
fn attack_and_release_go_63_percent_of_the_way_in_their_time() {
    let state = GateState {
        attack_ms: 5.0,
        hold_ms: 0.0,
        release_ms: 50.0,
        ..gate()
    };
    let signal = steps([(AT, quiet()), (2 * AT - 1, LOUD), (2 * SECOND, quiet())]);
    let output = Rig::new(state, signal).render(2 * SECOND);
    let floor = f64::from(decibels(-24.0));
    let part = 1.0 - (-1.0_f64).exp();
    let opened = f64::from(output[AT - 1 + 5 * MILLISECOND] / LOUD);
    let expected = floor + (1.0 - floor) * part;
    assert!((opened - expected).abs() < 1e-4, "{opened} {expected}");
    let last_open = (AT..2 * SECOND)
        .rev()
        .find(|frame| output[*frame] / quiet() == 1.0)
        .unwrap();
    let closed = f64::from(output[last_open + 50 * MILLISECOND] / quiet());
    let expected = floor + (1.0 - floor) * (1.0 - part);
    assert!((closed - expected).abs() < 1e-4, "{closed} {expected}");
}

/// A quiet pad under the threshold, keyed by loud hits on the sidechain: open while a hit
/// sounds, closed by the range once it has died away. With no key the pad is always closed.
#[test]
fn a_keyed_gate_opens_on_the_key() {
    let amplitude = decibels(-50.0);
    let state = GateState {
        release_ms: 10.0,
        ..gate()
    };
    let mut rig = Rig::new(state, sine(1_000.0, amplitude));
    rig.key(hits(SECOND / 2));
    let keyed = rig.render(SECOND);
    let unkeyed = Rig::new(state, sine(1_000.0, amplitude)).render(SECOND);
    let gain_db = |output: &[f32], from_ms: usize, to_ms: usize| {
        let window = &output[from_ms * SECOND / 1_000..to_ms * SECOND / 1_000];
        20.0 * (peak(window) / amplitude).log10()
    };
    // The hit is over the threshold for its first 170 ms.
    let open = gain_db(&keyed, 510, 650);
    assert!(open.abs() < 1e-3, "{open}");
    for closed in [gain_db(&keyed, 400, 500), gain_db(&unkeyed, 500, 1_000)] {
        assert!((closed + 24.0).abs() < 1e-3, "{closed}");
    }
}

/// Same project, same bytes: the gate and its shaper do not depend on where blocks start.
#[test]
fn every_block_size_renders_the_same_bytes() {
    let state = GateState {
        threshold_db: -30.0,
        transient_db: 6.0,
        sustain_db: -6.0,
        ..gate()
    };
    let render = |block| {
        let mut rig = Rig::new(state, hits(SECOND / 3));
        rig.key(hits(SECOND / 4));
        rig.render_in_blocks(SECOND, block)
    };
    let reference = render(512);
    for block in [64, 33, 480] {
        assert!(render(block) == reference, "{block}");
    }
}
