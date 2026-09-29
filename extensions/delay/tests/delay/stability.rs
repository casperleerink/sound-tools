//! Every setting is stable: nothing grows, also at the most feedback, nothing that is not a
//! number, the delay comes to rest, and a render is the same every time.

use delay::{DelayState, Division, FEEDBACK, Feel, HIGH_CUT, LOW_CUT, TIME};
use proptest::prelude::*;
use sound_core::Processor;

use crate::support::{Rig, SECOND, Signal, burst, db, noise, peak, rms, sine, tempo, wet};

/// The loudest the delay makes full scale noise at the most feedback. Each pass through the loop
/// is at most 0.95 of the one before at every frequency, so the level of the repeats of noise
/// settles at about three times the noise.
const BOUND: f32 = 12.0;

fn assert_bounded(label: &str, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    assert!(loudest < BOUND, "{label}: peak {loudest}");
}

/// Full scale noise for ten seconds at the most feedback, at every end of the cuts, from the
/// shortest time, side by side and in ping-pong: bounded, and as loud at the end as after five
/// seconds.
#[test]
fn full_scale_noise_at_the_most_feedback_stays_bounded_and_does_not_grow() {
    let mut loudest = 0.0_f32;
    // A repeat every 100 ms at most, so ten seconds are long enough to settle.
    for time_ms in [TIME.min, 20.0, 100.0] {
        for low_cut_hz in [LOW_CUT.min, 1_000.0] {
            for high_cut_hz in [2_000.0, HIGH_CUT.max] {
                for ping_pong in [false, true] {
                    let state = DelayState {
                        sync: false,
                        time_ms,
                        feedback: FEEDBACK.max,
                        low_cut_hz,
                        high_cut_hz,
                        ping_pong,
                        ..wet()
                    };
                    let label = format!("{state:?}");
                    let mut rig = Rig::new(state, noise(1.0));
                    rig.render(5 * SECOND);
                    let early = rig.render(SECOND);
                    rig.render(3 * SECOND);
                    let late = rig.render(SECOND);
                    assert_bounded(&label, &early);
                    assert_bounded(&label, &late);
                    // Each side: in ping-pong the right line is a path of its own.
                    for channel in 0..2 {
                        let growth = db(rms(&late[channel])) - db(rms(&early[channel]));
                        assert!(growth.abs() < 1.0, "{label}, {channel}: {growth:+.2} dB");
                    }
                    loudest = loudest.max(peak(&late[0])).max(peak(&late[1]));
                }
            }
        }
    }
    println!("loudest: {loudest:.2}");
}

/// A sample that is not a number, or is infinite, is silence or the limit to the lines. It
/// goes on as if nothing had happened: only the dry sound of those frames carries them.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_repeats() {
    const BROKEN: [usize; 3] = [1_000, 2_000, 3_000];
    let mut frame = 0_usize;
    let mut clean = sine(440.0, 0.5);
    let broken: Signal = Box::new(move || {
        frame += 1;
        match frame {
            1_000 => [f32::NAN, f32::INFINITY],
            2_000 => [f32::NEG_INFINITY, f32::NAN],
            3_000 => [f32::MAX, -f32::MAX],
            _ => clean(),
        }
    });
    let state = DelayState {
        feedback: FEEDBACK.max,
        ..DelayState::default()
    };
    let [left, right] = Rig::new(state, broken).render(4 * SECOND);
    for (index, (left, right)) in left.iter().zip(&right).enumerate() {
        if !BROKEN.contains(&(index + 1)) {
            assert!(left.is_finite() && right.is_finite(), "frame {index}");
        }
    }
}

/// Mix 0 is the sound as it came in, to the bit, also what the delay itself holds back.
#[test]
fn mix_zero_is_the_dry_sound_even_when_it_is_too_loud_for_the_lines() {
    let loud: Signal = Box::new(|| [100.0, -1e30]);
    let state = DelayState {
        mix: 0.0,
        ..DelayState::default()
    };
    let [left, right] = Rig::new(state, loud).render(4_800);
    assert!(left.iter().all(|sample| *sample == 100.0));
    assert!(right.iter().all(|sample| *sample == -1e30));

    let [left, right] = Rig::new(state, noise(0.5)).render(SECOND / 10);
    let mut dry = noise(0.5);
    for (left, right) in left.iter().zip(&right) {
        assert_eq!([*left, *right], dry());
    }
}

/// After the sound ends the repeats die away, also at the most feedback and with nothing cut,
/// and then the output is exactly silent: the delay does no work.
#[test]
fn after_the_sound_the_delay_comes_to_rest_and_is_silent() {
    let state = DelayState {
        division: Division::Sixteenth,
        feel: Feel::Triplet,
        feedback: FEEDBACK.max,
        ..wet()
    };
    let mut rig = Rig::new(state, burst(0.5, 4_800));
    let [ringing, _] = rig.render(SECOND);
    assert!(peak(&ringing[SECOND / 2..]) > 0.01);
    // At 0.95 a repeat every 83 ms falls by 180 dB in 34 s, and then the line has to empty.
    rig.render(40 * SECOND);
    let [rest, other] = rig.render(SECOND);
    assert!(rest.iter().chain(&other).all(|sample| *sample == 0.0));
}

#[test]
fn renders_are_the_same_every_time_to_the_byte() {
    let state = DelayState {
        feedback: 0.7,
        ping_pong: true,
        mix: 0.6,
        ..DelayState::default()
    };
    let render = || {
        let mut rig = Rig::new(state, burst(0.8, SECOND / 2));
        let first = rig.render(SECOND);
        rig.update(DelayState {
            sync: false,
            time_ms: 180.0,
            ping_pong: false,
            ..state
        });
        rig.control.set_tempo_map(tempo(97.0));
        let [left, right] = rig.render(SECOND);
        let bytes = |samples: &[f32]| -> Vec<u8> {
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect()
        };
        [first[0].clone(), first[1].clone(), left, right].map(|samples| bytes(&samples))
    };
    assert_eq!(render(), render());
}

#[test]
fn the_delay_has_no_latency() {
    assert_eq!(delay::Delay::new(DelayState::default()).latency(), 0);
}

fn any_state() -> impl Strategy<Value = DelayState> {
    (
        any::<bool>(),
        0..Division::ALL.len(),
        0..Feel::ALL.len(),
        TIME.min..=TIME.max,
        FEEDBACK.min..=FEEDBACK.max,
        any::<bool>(),
        LOW_CUT.min..=LOW_CUT.max,
        HIGH_CUT.min..=HIGH_CUT.max,
    )
        .prop_map(
            |(sync, division, feel, time_ms, feedback, ping_pong, low_cut_hz, high_cut_hz)| {
                DelayState {
                    sync,
                    division: Division::ALL[division],
                    feel: Feel::ALL[feel],
                    time_ms,
                    feedback,
                    ping_pong,
                    low_cut_hz,
                    high_cut_hz,
                    mix: 1.0,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Any state at any tempo, and then another state at another tempo while the noise plays:
    /// every sample is a number and under the bound.
    #[test]
    fn any_state_and_any_edit_stays_bounded(
        first in any_state(),
        second in any_state(),
        bpm in 10.0..=1_000.0_f64,
        other_bpm in 10.0..=1_000.0_f64,
    ) {
        let mut rig = Rig::new(first, noise(1.0));
        rig.control.set_tempo_map(tempo(bpm));
        assert_bounded("first", &rig.render(SECOND));
        rig.update(second);
        rig.control.set_tempo_map(tempo(other_bpm));
        assert_bounded("second", &rig.render(SECOND));
    }
}
