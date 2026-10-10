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

/// A constant that steps from loud to under the threshold: the gate stays open, gain exactly
/// 1, while the loud part is in the detector (10 to 11 ms) and for the hold after, then closes.
#[test]
fn the_gate_stays_open_for_the_hold_after_the_level_falls() {
    let (loud, quiet, at) = (0.5_f32, decibels(-60.0), SECOND / 2);
    let step = || -> Signal {
        let mut frame = 0;
        Box::new(move || {
            frame += 1;
            [if frame <= at { loud } else { quiet }; 2]
        })
    };
    let millisecond = SECOND / 1_000;
    for hold_ms in [0.0, 50.0] {
        let state = GateState {
            hold_ms,
            release_ms: 1.0,
            ..gate()
        };
        let output = Rig::new(state, step()).render(SECOND);
        let gain = |frame: usize| output[frame] / quiet;
        let open_until = at - 1 + 10 * millisecond + hold_ms as usize * millisecond;
        assert_eq!(gain(open_until), 1.0, "hold {hold_ms}");
        let closed = gain(open_until + 12 * millisecond);
        assert!(
            (closed - decibels(-24.0)).abs() < 1e-3,
            "hold {hold_ms}: {closed}"
        );
    }
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
