//! The same render every time, live or offline, and a loop of the song.

use pitch_correction::PitchCorrectionState;
use sound_core::Ticks;

use crate::support::{Rig, SAMPLE_RATE, between, farthest_cents, heard, held, hz, onsets, voice};

const SECOND: usize = SAMPLE_RATE as usize;

/// A phrase that slides, holds and stops, under notes held for part of it.
fn render(offline: bool, buffer: usize) -> Vec<f32> {
    let phrase = |time: f64| match time % 1.0 {
        part if part < 0.3 => Some(57.3 + part),
        part if part < 0.8 => Some(58.6 + 0.2 * (std::f64::consts::TAU * 5.0 * part).sin()),
        _ => None,
    };
    let state = PitchCorrectionState {
        retune_ms: 30.0,
        humanize: 0.5,
        keep_formants: true,
        ..PitchCorrectionState::default()
    };
    let notes = held(62, 1.2, 1.6).to_vec();
    let mut rig = Rig::with(state, voice(phrase), false, notes, offline);
    rig.render_in(3 * SECOND, buffer)
}

#[test]
fn a_render_is_the_same_every_time_live_or_offline_in_any_buffers() {
    let expected = render(true, 480);
    assert!(expected.iter().any(|sample| sample.abs() > 0.1));
    for (offline, buffer) in [(true, 480), (false, 128), (true, 1_000)] {
        let again = render(offline, buffer);
        let differs = (expected.iter().zip(&again)).position(|(expected, again)| expected != again);
        assert_eq!(differs, None, "offline {offline}, in buffers of {buffer}");
    }
}

#[test]
fn each_pass_of_a_loop_is_in_time_and_in_tune() {
    // A3 sharp from 0.1 s to 0.6 s of every second of the song, which loops over its first
    // second: two beats at 120 bpm.
    let phrase = |time: f64| (0.1..0.6).contains(&(time % 1.0)).then_some(57.3);
    let state = PitchCorrectionState {
        retune_ms: 0.0,
        ..PitchCorrectionState::default()
    };
    let mut rig = Rig::with(state, voice(phrase), true, Vec::new(), true);
    rig.play_looped(Ticks(0), Ticks(1_920));
    let output = rig.render(4 * SECOND);
    let onsets = onsets(&output);
    assert_eq!(onsets.len(), 4, "{onsets:?}");
    // Each start within 0.6 of a period of where it belongs, as the head stays.
    let window = (0.6 * f64::from(SAMPLE_RATE) / hz(57.3)).ceil() as i64;
    for pair in onsets.windows(2) {
        let pass = pair[1] as i64 - pair[0] as i64;
        assert!(
            (pass - SECOND as i64).abs() <= 2 * window,
            "a pass of {pass} frames"
        );
    }
    // The song is not late: the project makes up for the latency.
    let heard = heard(&output, 0);
    for onset in onsets {
        let start = onset as f64 / SECOND as f64;
        let notes = between(&heard, start + 0.03, start + 0.45);
        assert!(
            farthest_cents(&notes, 57.0) < 3.0,
            "at {start} s: {notes:?}"
        );
    }
}
