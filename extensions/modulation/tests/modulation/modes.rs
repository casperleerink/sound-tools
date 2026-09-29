//! What each mode does to a tone while the LFO moves: the chorus bends its pitch up and down by
//! as much as its delay moves, the flanger and the phaser sweep a notch across it.

use std::f64::consts::{LN_2, TAU};

use modulation::{Mode, ModulationState, sweep};

use crate::support::{Rig, SECOND, db, frequencies, rms, sine};

/// A copy of a tone read from a delay that moves plays at `1 - d'(t)`: the pitch follows how
/// fast the delay moves. The delay of the chorus is `centre 2^(k sin θ)` with `k` its octaves
/// times the depth, so it moves by `centre k ln 2 2π rate cos θ 2^(k sin θ)`, fastest a little
/// above its centre.
#[test]
fn the_chorus_bends_the_pitch_of_its_copy_by_as_much_as_its_delay_moves() {
    const HZ: f64 = 1_000.0;
    let state = ModulationState {
        mode: Mode::Chorus,
        rate_hz: 1.0,
        depth: 1.0,
        feedback: 0.0,
        mix: 1.0,
        ..ModulationState::default()
    };
    let [low, high] = sweep(&state);
    let (centre, octaves) = (
        f64::from((low * high).sqrt()),
        f64::from((high / low).log2() / 2.0),
    );
    let k = octaves * f64::from(state.depth);
    let fastest = (0..3_600)
        .map(|step| TAU * f64::from(step) / 3_600.0)
        .map(|angle| angle.cos() * (k * angle.sin()).exp2())
        .fold(0.0, f64::max);
    let expected = centre / 1_000.0 * k * LN_2 * TAU * f64::from(state.rate_hz) * fastest;

    let [left, _] = Rig::new(state, sine(HZ, 0.5)).render(3 * SECOND);
    // After the first copy has come out of the line.
    let heard = frequencies(&left[SECOND..]);
    let (lowest, highest) = heard
        .iter()
        .fold((f64::MAX, 0.0_f64), |(low, high), (_, hz)| {
            (low.min(*hz), high.max(*hz))
        });
    let (down, up) = (1.0 - lowest / HZ, highest / HZ - 1.0);
    println!("expected ±{expected:.4}, heard -{down:.4} +{up:.4}");
    for bend in [down, up] {
        assert!(
            (bend - expected).abs() < 0.1 * expected,
            "{bend} {expected}"
        );
    }

    // At depth 0 the copy holds its pitch.
    let still = ModulationState {
        depth: 0.0,
        ..state
    };
    let [left, _] = Rig::new(still, sine(HZ, 0.5)).render(2 * SECOND);
    for (_, hz) in frequencies(&left[SECOND..]) {
        assert!((hz - HZ).abs() < 0.01, "{hz}");
    }
}

/// The level of a tone of amplitude 0.5 against its own level, in windows of 10 ms over one
/// whole LFO cycle and one more window.
fn levels_at_the_notch(mode: Mode, hz: f64) -> Vec<f64> {
    let state = ModulationState {
        mode,
        rate_hz: 0.1,
        depth: 0.5,
        feedback: 0.0,
        mix: 0.5,
        ..ModulationState::default()
    };
    let [left, _] = Rig::new(state, sine(hz, 0.5)).render(10 * SECOND + SECOND / 100);
    let reference = 0.5 / std::f64::consts::SQRT_2;
    left.chunks(SECOND / 100)
        .map(|window| db(rms(window) / reference))
        .collect()
}

/// A tone at the notch of the middle of the sweep is gone as the LFO passes its middle, at 5 s
/// and 10 s, and back as the notch moves away from it, at 2.5 s and 7.5 s.
#[test]
fn the_notches_of_the_flanger_and_the_phaser_sweep_across_a_tone() {
    for (mode, hz) in [(Mode::Flanger, 1_000.0 / 3.0), (Mode::Phaser, 1_000.0)] {
        let levels = levels_at_the_notch(mode, hz);
        let at = |seconds: f64| levels[(seconds * 100.0) as usize];
        println!(
            "{mode:?}: {:.1} dB at 2.5 s, {:.1} at 5 s, {:.1} at 7.5 s, {:.1} at 10 s",
            at(2.5),
            at(5.0),
            at(7.5),
            at(10.0)
        );
        assert!(at(5.0) < -35.0, "{mode:?}");
        assert!(at(10.0) < -35.0, "{mode:?}");
        assert!(at(2.5) > -6.0, "{mode:?}");
        assert!(at(7.5) > -6.0, "{mode:?}");
    }
}
